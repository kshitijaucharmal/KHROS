// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2023 Andre Richter <andre.o.richter@gmail.com>

//! GPIO Driver.

use crate::{
    bsp::device_driver::common::MMIODerefWrapper,
    driver,
    exception::asynchronous::IRQNumber,
    memory::{Address, Virtual},
    synchronization,
    synchronization::IRQSafeNullLock,
};
use tock_registers::{
    interfaces::{ReadWriteable, Writeable},
    register_bitfields, register_structs,
    registers::{ReadOnly, ReadWrite, WriteOnly},
};

use core::ptr;

const GPIO_FSEL0: u32 = 0x3F20_0000;
const GPIO_FSEL1: u32 = 0x3F20_0004;
const GPIO_FSEL2: u32 = 0x3F20_0008;
const GPIO_SETO: u32 = 0x3F20_001C;
const GPIO_CLRO: u32 = 0x3F20_0028;

const GPIO_LEV0: u32 = 0x3F20_0034;
const GPIO_LEV1: u32 = 0x3F20_0038;

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

// GPIO registers.
//
// Descriptions taken from
// - https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf
// - https://datasheets.raspberrypi.org/bcm2711/bcm2711-peripherals.pdf
register_bitfields! {
    u32,

    GPFSEL0 [
        FSEL0 OFFSET(0)  NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL1 OFFSET(3)  NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL2 OFFSET(6)  NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL3 OFFSET(9)  NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL4 OFFSET(12) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL5 OFFSET(15) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL6 OFFSET(18) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL7 OFFSET(21) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL8 OFFSET(24) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL9 OFFSET(27) NUMBITS(3) [ Input = 0b000, Output = 0b001],
    ],

    /// GPIO Function Select 1
    GPFSEL1 [
        FSEL10 OFFSET(0)  NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ],
        FSEL11 OFFSET(3)  NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ],
        FSEL12 OFFSET(6)  NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ],
        FSEL13 OFFSET(9)  NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ],
        /// Pin 14 AltFunc0 PL011 UART TX
        FSEL14 OFFSET(12) NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ],
        /// Pin 15 AltFunc0 PL011 UART RX
        FSEL15 OFFSET(15) NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ],
        FSEL16 OFFSET(18) NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ],
        FSEL17 OFFSET(21) NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ],
        FSEL18 OFFSET(24) NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ],
        FSEL19 OFFSET(27) NUMBITS(3) [ Input = 0b000, Output = 0b001, AltFunc0 = 0b100 ]
    ],

    GPFSEL2 [
        FSEL20 OFFSET(0)  NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL21 OFFSET(3)  NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL22 OFFSET(6)  NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL23 OFFSET(9)  NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL24 OFFSET(12) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL25 OFFSET(15) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL26 OFFSET(18) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL27 OFFSET(21) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL28 OFFSET(24) NUMBITS(3) [ Input = 0b000, Output = 0b001],
        FSEL29 OFFSET(27) NUMBITS(3) [ Input = 0b000, Output = 0b001]
    ],

    /// GPIO Pull-up/down Register
    ///
    /// BCM2837 only.
    GPPUD [
        /// Controls the actuation of the internal pull-up/down control line to ALL the GPIO pins.
        PUD OFFSET(0) NUMBITS(2) [
            Off = 0b00,
            PullDown = 0b01,
            PullUp = 0b10
        ]
    ],

    /// GPIO Pull-up/down Clock Register 0
    ///
    /// BCM2837 only.
    GPPUDCLK0 [
        /// Pin 15
        PUDCLK15 OFFSET(15) NUMBITS(1) [
            NoEffect = 0,
            AssertClock = 1
        ],

        /// Pin 14
        PUDCLK14 OFFSET(14) NUMBITS(1) [
            NoEffect = 0,
            AssertClock = 1
        ]
    ],

    /// GPIO Pull-up / Pull-down Register 0
    ///
    /// BCM2711 only.
    GPIO_PUP_PDN_CNTRL_REG0 [
        /// Pin 15
        GPIO_PUP_PDN_CNTRL15 OFFSET(30) NUMBITS(2) [
            NoResistor = 0b00,
            PullUp = 0b01
        ],

        /// Pin 14
        GPIO_PUP_PDN_CNTRL14 OFFSET(28) NUMBITS(2) [
            NoResistor = 0b00,
            PullUp = 0b01
        ]
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    RegisterBlock {
        (0x00 => GPFSEL0: ReadWrite<u32, GPFSEL0::Register>),
        (0x04 => GPFSEL1: ReadWrite<u32, GPFSEL1::Register>),
        (0x08 => GPFSEL2: ReadWrite<u32, GPFSEL2::Register>),
        (0x0C => _reserved2),
        (0x1C => GPSET0: WriteOnly<u32>),   // Set GPIO 0–31
        (0x20 => GPSET1: WriteOnly<u32>),   // Set GPIO 32–53
        (0x24 => _reserved3),               // 0x24 is reserved (not used)
        (0x28 => GPCLR0: WriteOnly<u32>),   // Clear GPIO 0–31
        (0x2C => GPCLR1: WriteOnly<u32>),   // Clear GPIO 32–53
        (0x30 => _reserved4),               // 0x30 reserved
        (0x94 => GPPUD: ReadWrite<u32, GPPUD::Register>),
        (0x98 => GPPUDCLK0: ReadWrite<u32, GPPUDCLK0::Register>),
        (0x9C => _reserved5),
        (0xE4 => GPIO_PUP_PDN_CNTRL_REG0: ReadWrite<u32, GPIO_PUP_PDN_CNTRL_REG0::Register>),
        (0xE8 => @END),
    }
}

/// Abstraction for the associated MMIO registers.
type Registers = MMIODerefWrapper<RegisterBlock>;

struct GPIOInner {
    registers: Registers,
}

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Representation of the GPIO HW.
pub struct GPIO {
    inner: IRQSafeNullLock<GPIOInner>,
}

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

impl GPIOInner {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(mmio_start_addr: Address<Virtual>) -> Self {
        Self {
            registers: Registers::new(mmio_start_addr),
        }
    }

    /// Disable pull-up/down on pins 14 and 15.
    #[cfg(feature = "bsp_rpi3")]
    fn disable_pud_14_15_bcm2837(&mut self) {
        use crate::time;
        use core::time::Duration;

        // The Linux 2837 GPIO driver waits 1 µs between the steps.
        const DELAY: Duration = Duration::from_micros(1);

        self.registers.GPPUD.write(GPPUD::PUD::Off);
        time::time_manager().spin_for(DELAY);

        self.registers
            .GPPUDCLK0
            .write(GPPUDCLK0::PUDCLK15::AssertClock + GPPUDCLK0::PUDCLK14::AssertClock);
        time::time_manager().spin_for(DELAY);

        self.registers.GPPUD.write(GPPUD::PUD::Off);
        self.registers.GPPUDCLK0.set(0);
    }

    /// Disable pull-up/down on pins 14 and 15.
    #[cfg(feature = "bsp_rpi4")]
    fn disable_pud_14_15_bcm2711(&mut self) {
        self.registers.GPIO_PUP_PDN_CNTRL_REG0.write(
            GPIO_PUP_PDN_CNTRL_REG0::GPIO_PUP_PDN_CNTRL15::PullUp
                + GPIO_PUP_PDN_CNTRL_REG0::GPIO_PUP_PDN_CNTRL14::PullUp,
        );
    }

    /// Map PL011 UART as standard output.
    ///
    /// TX to pin 14
    /// RX to pin 15
    pub fn map_pl011_uart(&mut self) {
        // Select the UART on pins 14 and 15.
        self.registers
            .GPFSEL1
            .modify(GPFSEL1::FSEL15::AltFunc0 + GPFSEL1::FSEL14::AltFunc0);

        // Disable pull-up/down on pins 14 and 15.
        #[cfg(feature = "bsp_rpi3")]
        self.disable_pud_14_15_bcm2837();

        #[cfg(feature = "bsp_rpi4")]
        self.disable_pud_14_15_bcm2711();
    }

    pub fn set_gpio17_as_output(&self) {
        self.registers.GPFSEL1.modify(GPFSEL1::FSEL17::Output);
    }

    pub fn set_pin_as_output(&self, pin: u8) {
        assert!(pin <= 29, "Only GPIO 0–29 are supported");

        use GPFSEL0::*;
        use GPFSEL1::*;
        use GPFSEL2::*;

        match pin {
            0 => self.registers.GPFSEL0.modify(FSEL0::Output),
            1 => self.registers.GPFSEL0.modify(FSEL1::Output),
            2 => self.registers.GPFSEL0.modify(FSEL2::Output),
            3 => self.registers.GPFSEL0.modify(FSEL3::Output),
            4 => self.registers.GPFSEL0.modify(FSEL4::Output),
            5 => self.registers.GPFSEL0.modify(FSEL5::Output),
            6 => self.registers.GPFSEL0.modify(FSEL6::Output),
            7 => self.registers.GPFSEL0.modify(FSEL7::Output),
            8 => self.registers.GPFSEL0.modify(FSEL8::Output),
            9 => self.registers.GPFSEL0.modify(FSEL9::Output),

            10 => self.registers.GPFSEL1.modify(FSEL10::Output),
            11 => self.registers.GPFSEL1.modify(FSEL11::Output),
            12 => self.registers.GPFSEL1.modify(FSEL12::Output),
            13 => self.registers.GPFSEL1.modify(FSEL13::Output),
            14 => self.registers.GPFSEL1.modify(FSEL14::Output),
            15 => self.registers.GPFSEL1.modify(FSEL15::Output),
            16 => self.registers.GPFSEL1.modify(FSEL16::Output),
            17 => self.registers.GPFSEL1.modify(FSEL17::Output),
            18 => self.registers.GPFSEL1.modify(FSEL18::Output),
            19 => self.registers.GPFSEL1.modify(FSEL19::Output),

            20 => self.registers.GPFSEL2.modify(FSEL20::Output),
            21 => self.registers.GPFSEL2.modify(FSEL21::Output),
            22 => self.registers.GPFSEL2.modify(FSEL22::Output),
            23 => self.registers.GPFSEL2.modify(FSEL23::Output),
            24 => self.registers.GPFSEL2.modify(FSEL24::Output),
            25 => self.registers.GPFSEL2.modify(FSEL25::Output),
            26 => self.registers.GPFSEL2.modify(FSEL26::Output),
            27 => self.registers.GPFSEL2.modify(FSEL27::Output),
            28 => self.registers.GPFSEL2.modify(FSEL28::Output),
            29 => self.registers.GPFSEL2.modify(FSEL29::Output),

            _ => panic!("Unsupported GPIO pin {pin}"),
        }
    }
    pub fn set_gpio_high(&self, pin: u8) {
        assert!(pin <= 29, "Only GPIO 0–29 are supported");
        if pin < 32 {
            self.registers.GPSET0.set(1 << pin);
        } else {
            self.registers.GPSET1.set(1 << (pin - 32));
        }
    }
    pub fn set_gpio_low(&self, pin: u8) {
        assert!(pin <= 29, "Only GPIO 0–29 are supported");
        if pin < 32 {
            self.registers.GPCLR0.set(1 << pin);
        } else {
            self.registers.GPCLR1.set(1 << (pin - 32));
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl GPIO {
    pub const COMPATIBLE: &'static str = "BCM GPIO";

    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(mmio_start_addr: Address<Virtual>) -> Self {
        Self {
            inner: IRQSafeNullLock::new(GPIOInner::new(mmio_start_addr)),
        }
    }

    /// Concurrency safe version of `GPIOInner.map_pl011_uart()`
    pub fn map_pl011_uart(&self) {
        self.inner.lock(|inner| inner.map_pl011_uart())
    }

    pub fn set_pin_as_output(&self, pin: u8) {
        self.inner.lock(|inner| inner.set_pin_as_output(pin))
    }
    pub fn set_gpio_high(&self, pin: u8) {
        self.inner.lock(|inner| inner.set_gpio_high(pin))
    }
    pub fn set_gpio_low(&self, pin: u8) {
        self.inner.lock(|inner| inner.set_gpio_low(pin))
    }
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------
use synchronization::interface::Mutex;

impl driver::interface::DeviceDriver for GPIO {
    type IRQNumberType = IRQNumber;

    fn compatible(&self) -> &'static str {
        Self::COMPATIBLE
    }
}
