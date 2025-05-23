//! PL011 UART driver.
//!
//! # Resources
//!
//! - <https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf>
//! - <https://developer.arm.com/documentation/ddi0183/latest>

use crate::{
    bsp::{device_driver::common::MMIODerefWrapper, driver::gpio_high},
    common, console, cpu, driver,
    exception::{self, asynchronous::IRQNumber},
    info,
    memory::{Address, Virtual},
    synchronization::{self, IRQSafeNullLock},
};
use alloc::{boxed::Box, vec::Vec};
use core::{arch::asm, fmt, time::Duration};
use tock_registers::{
    interfaces::{Readable, Writeable},
    register_bitfields, register_structs,
    registers::{ReadOnly, ReadWrite, WriteOnly},
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

const CMD_BUF_CAPACITY: usize = 64;

// PL011 UART registers.
//
// Descriptions taken from "PrimeCell UART (PL011) Technical Reference Manual" r1p5.
register_bitfields! {
    u32,

    /// Flag Register.
    FR [
        /// Transmit FIFO empty. The meaning of this bit depends on the state of the FEN bit in the
        /// Line Control Register, LCR_H.
        ///
        /// - If the FIFO is disabled, this bit is set when the transmit holding register is empty.
        /// - If the FIFO is enabled, the TXFE bit is set when the transmit FIFO is empty.
        /// - This bit does not indicate if there is data in the transmit shift register.
        TXFE OFFSET(7) NUMBITS(1) [],

        /// Transmit FIFO full. The meaning of this bit depends on the state of the FEN bit in the
        /// LCR_H Register.
        ///
        /// - If the FIFO is disabled, this bit is set when the transmit holding register is full.
        /// - If the FIFO is enabled, the TXFF bit is set when the transmit FIFO is full.
        TXFF OFFSET(5) NUMBITS(1) [],

        /// Receive FIFO empty. The meaning of this bit depends on the state of the FEN bit in the
        /// LCR_H Register.
        ///
        /// - If the FIFO is disabled, this bit is set when the receive holding register is empty.
        /// - If the FIFO is enabled, the RXFE bit is set when the receive FIFO is empty.
        RXFE OFFSET(4) NUMBITS(1) [],

        /// UART busy. If this bit is set to 1, the UART is busy transmitting data. This bit remains
        /// set until the complete byte, including all the stop bits, has been sent from the shift
        /// register.
        ///
        /// This bit is set as soon as the transmit FIFO becomes non-empty, regardless of whether
        /// the UART is enabled or not.
        BUSY OFFSET(3) NUMBITS(1) []
    ],

    /// Integer Baud Rate Divisor.
    IBRD [
        /// The integer baud rate divisor.
        BAUD_DIVINT OFFSET(0) NUMBITS(16) []
    ],

    /// Fractional Baud Rate Divisor.
    FBRD [
        ///  The fractional baud rate divisor.
        BAUD_DIVFRAC OFFSET(0) NUMBITS(6) []
    ],

    /// Line Control Register.
    LCR_H [
        /// Word length. These bits indicate the number of data bits transmitted or received in a
        /// frame.
        #[allow(clippy::enum_variant_names)]
        WLEN OFFSET(5) NUMBITS(2) [
            FiveBit = 0b00,
            SixBit = 0b01,
            SevenBit = 0b10,
            EightBit = 0b11
        ],

        /// Enable FIFOs:
        ///
        /// 0 = FIFOs are disabled (character mode) that is, the FIFOs become 1-byte-deep holding
        /// registers.
        ///
        /// 1 = Transmit and receive FIFO buffers are enabled (FIFO mode).
        FEN  OFFSET(4) NUMBITS(1) [
            FifosDisabled = 0,
            FifosEnabled = 1
        ]
    ],

    /// Control Register.
    CR [
        /// Receive enable. If this bit is set to 1, the receive section of the UART is enabled.
        /// Data reception occurs for either UART signals or SIR signals depending on the setting of
        /// the SIREN bit. When the UART is disabled in the middle of reception, it completes the
        /// current character before stopping.
        RXE OFFSET(9) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// Transmit enable. If this bit is set to 1, the transmit section of the UART is enabled.
        /// Data transmission occurs for either UART signals, or SIR signals depending on the
        /// setting of the SIREN bit. When the UART is disabled in the middle of transmission, it
        /// completes the current character before stopping.
        TXE OFFSET(8) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// UART enable:
        ///
        /// 0 = UART is disabled. If the UART is disabled in the middle of transmission or
        /// reception, it completes the current character before stopping.
        ///
        /// 1 = The UART is enabled. Data transmission and reception occurs for either UART signals
        /// or SIR signals depending on the setting of the SIREN bit
        UARTEN OFFSET(0) NUMBITS(1) [
            /// If the UART is disabled in the middle of transmission or reception, it completes the
            /// current character before stopping.
            Disabled = 0,
            Enabled = 1
        ]
    ],

    /// Interrupt FIFO Level Select Register.
    IFLS [
        /// Receive interrupt FIFO level select. The trigger points for the receive interrupt are as
        /// follows.
        RXIFLSEL OFFSET(3) NUMBITS(5) [
            OneEigth = 0b000,
            OneQuarter = 0b001,
            OneHalf = 0b010,
            ThreeQuarters = 0b011,
            SevenEights = 0b100
        ]
    ],

    /// Interrupt Mask Set/Clear Register.
    IMSC [
        /// Receive timeout interrupt mask. A read returns the current mask for the UARTRTINTR
        /// interrupt.
        ///
        /// - On a write of 1, the mask of the UARTRTINTR interrupt is set.
        /// - A write of 0 clears the mask.
        RTIM OFFSET(6) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// Receive interrupt mask. A read returns the current mask for the UARTRXINTR interrupt.
        ///
        /// - On a write of 1, the mask of the UARTRXINTR interrupt is set.
        /// - A write of 0 clears the mask.
        RXIM OFFSET(4) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ]
    ],

    /// Masked Interrupt Status Register.
    MIS [
        /// Receive timeout masked interrupt status. Returns the masked interrupt state of the
        /// UARTRTINTR interrupt.
        RTMIS OFFSET(6) NUMBITS(1) [],

        /// Receive masked interrupt status. Returns the masked interrupt state of the UARTRXINTR
        /// interrupt.
        RXMIS OFFSET(4) NUMBITS(1) []
    ],

    /// Interrupt Clear Register.
    ICR [
        /// Meta field for all pending interrupts.
        ALL OFFSET(0) NUMBITS(11) []
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    pub RegisterBlock {
        (0x00 => DR: ReadWrite<u32>),
        (0x04 => _reserved1),
        (0x18 => FR: ReadOnly<u32, FR::Register>),
        (0x1c => _reserved2),
        (0x24 => IBRD: WriteOnly<u32, IBRD::Register>),
        (0x28 => FBRD: WriteOnly<u32, FBRD::Register>),
        (0x2c => LCR_H: WriteOnly<u32, LCR_H::Register>),
        (0x30 => CR: WriteOnly<u32, CR::Register>),
        (0x34 => IFLS: ReadWrite<u32, IFLS::Register>),
        (0x38 => IMSC: ReadWrite<u32, IMSC::Register>),
        (0x3C => _reserved3),
        (0x40 => MIS: ReadOnly<u32, MIS::Register>),
        (0x44 => ICR: WriteOnly<u32, ICR::Register>),
        (0x48 => @END),
    }
}

/// Abstraction for the associated MMIO registers.
type Registers = MMIODerefWrapper<RegisterBlock>;

#[derive(PartialEq)]
enum BlockingMode {
    Blocking,
    NonBlocking,
}

struct PL011UartInner {
    registers: Registers,
    chars_written: usize,
    chars_read: usize,
    cmd_buf: [u8; CMD_BUF_CAPACITY],
    cmd_len: usize,
}

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Representation of the UART.
pub struct PL011Uart {
    inner: IRQSafeNullLock<PL011UartInner>,
}

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

impl PL011UartInner {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(mmio_start_addr: Address<Virtual>) -> Self {
        Self {
            registers: Registers::new(mmio_start_addr),
            chars_written: 0,
            chars_read: 0,
            cmd_buf: [0; 64],
            cmd_len: 0,
        }
    }

    /// Set up baud rate and characteristics.
    ///
    /// This results in 8N1 and 921_600 baud.
    ///
    /// The calculation for the BRD is (we set the clock to 48 MHz in config.txt):
    /// `(48_000_000 / 16) / 921_600 = 3.2552083`.
    ///
    /// This means the integer part is `3` and goes into the `IBRD`.
    /// The fractional part is `0.2552083`.
    ///
    /// `FBRD` calculation according to the PL011 Technical Reference Manual:
    /// `INTEGER((0.2552083 * 64) + 0.5) = 16`.
    ///
    /// Therefore, the generated baud rate divider is: `3 + 16/64 = 3.25`. Which results in a
    /// genrated baud rate of `48_000_000 / (16 * 3.25) = 923_077`.
    ///
    /// Error = `((923_077 - 921_600) / 921_600) * 100 = 0.16%`.
    pub fn init(&mut self) {
        // Execution can arrive here while there are still characters queued in the TX FIFO and
        // actively being sent out by the UART hardware. If the UART is turned off in this case,
        // those queued characters would be lost.
        //
        // For example, this can happen during runtime on a call to panic!(), because panic!()
        // initializes its own UART instance and calls init().
        //
        // Hence, flush first to ensure all pending characters are transmitted.
        self.flush();

        // Turn the UART off temporarily.
        self.registers.CR.set(0);

        // Clear all pending interrupts.
        self.registers.ICR.write(ICR::ALL::CLEAR);

        // From the PL011 Technical Reference Manual:
        //
        // The LCR_H, IBRD, and FBRD registers form the single 30-bit wide LCR Register that is
        // updated on a single write strobe generated by a LCR_H write. So, to internally update the
        // contents of IBRD or FBRD, a LCR_H write must always be performed at the end.
        //
        // Set the baud rate, 8N1 and FIFO enabled.
        self.registers.IBRD.write(IBRD::BAUD_DIVINT.val(3));
        self.registers.FBRD.write(FBRD::BAUD_DIVFRAC.val(16));
        self.registers
            .LCR_H
            .write(LCR_H::WLEN::EightBit + LCR_H::FEN::FifosEnabled);

        // Set RX FIFO fill level at 1/8.
        self.registers.IFLS.write(IFLS::RXIFLSEL::OneEigth);

        // Enable RX IRQ + RX timeout IRQ.
        self.registers
            .IMSC
            .write(IMSC::RXIM::Enabled + IMSC::RTIM::Enabled);

        // Turn the UART on.
        self.registers
            .CR
            .write(CR::UARTEN::Enabled + CR::TXE::Enabled + CR::RXE::Enabled);
    }

    /// Send a character.
    fn write_char(&mut self, c: char) {
        // Spin while TX FIFO full is set, waiting for an empty slot.
        while self.registers.FR.matches_all(FR::TXFF::SET) {
            cpu::nop();
        }

        // Write the character to the buffer.
        self.registers.DR.set(c as u32);

        self.chars_written += 1;
    }

    /// Send a slice of characters.
    fn write_array(&mut self, a: &[char]) {
        for c in a {
            self.write_char(*c);
        }
    }

    /// Block execution until the last buffered character has been physically put on the TX wire.
    fn flush(&self) {
        // Spin until the busy bit is cleared.
        while self.registers.FR.matches_all(FR::BUSY::SET) {
            cpu::nop();
        }
    }

    /// Retrieve a character.
    fn read_char_converting(&mut self, blocking_mode: BlockingMode) -> Option<char> {
        // If RX FIFO is empty,
        if self.registers.FR.matches_all(FR::RXFE::SET) {
            // immediately return in non-blocking mode.
            if blocking_mode == BlockingMode::NonBlocking {
                return None;
            }

            // Otherwise, wait until a char was received.
            while self.registers.FR.matches_all(FR::RXFE::SET) {
                cpu::nop();
            }
        }

        // Read one character.
        let mut ret = self.registers.DR.get() as u8 as char;

        // Convert carrige return to newline.
        if ret == '\r' {
            ret = '\n'
        }

        // Update statistics.
        self.chars_read += 1;

        Some(ret)
    }
}

/// Implementing `core::fmt::Write` enables usage of the `format_args!` macros, which in turn are
/// used to implement the `kernel`'s `print!` and `println!` macros. By implementing `write_str()`,
/// we get `write_fmt()` automatically.
///
/// The function takes an `&mut self`, so it must be implemented for the inner struct.
///
/// See [`src/print.rs`].
///
/// [`src/print.rs`]: ../../print/index.html
impl fmt::Write for PL011UartInner {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }

        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl PL011Uart {
    pub const COMPATIBLE: &'static str = "BCM PL011 UART";

    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(mmio_start_addr: Address<Virtual>) -> Self {
        Self {
            inner: IRQSafeNullLock::new(PL011UartInner::new(mmio_start_addr)),
        }
    }
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------
use synchronization::interface::Mutex;

impl driver::interface::DeviceDriver for PL011Uart {
    type IRQNumberType = IRQNumber;

    fn compatible(&self) -> &'static str {
        Self::COMPATIBLE
    }

    unsafe fn init(&self) -> Result<(), &'static str> {
        self.inner.lock(|inner| inner.init());

        Ok(())
    }

    fn register_and_enable_irq_handler(
        &'static self,
        irq_number: &Self::IRQNumberType,
    ) -> Result<(), &'static str> {
        use exception::asynchronous::{irq_manager, IRQHandlerDescriptor};

        let descriptor = IRQHandlerDescriptor::new(*irq_number, Self::COMPATIBLE, self);

        irq_manager().register_handler(descriptor)?;
        irq_manager().enable(irq_number);

        Ok(())
    }
}

impl console::interface::Write for PL011Uart {
    /// Passthrough of `args` to the `core::fmt::Write` implementation, but guarded by a Mutex to
    /// serialize access.
    fn write_char(&self, c: char) {
        self.inner.lock(|inner| inner.write_char(c));
    }

    fn write_array(&self, a: &[char]) {
        self.inner.lock(|inner| inner.write_array(a));
    }

    fn write_fmt(&self, args: core::fmt::Arguments) -> fmt::Result {
        // Fully qualified syntax for the call to `core::fmt::Write::write_fmt()` to increase
        // readability.
        self.inner.lock(|inner| fmt::Write::write_fmt(inner, args))
    }

    fn flush(&self) {
        // Spin until TX FIFO empty is set.
        self.inner.lock(|inner| inner.flush());
    }
}

impl console::interface::Read for PL011Uart {
    fn read_char(&self) -> char {
        self.inner
            .lock(|inner| inner.read_char_converting(BlockingMode::Blocking).unwrap())
    }

    fn clear_rx(&self) {
        // Read from the RX FIFO until it is indicating empty.
        while self
            .inner
            .lock(|inner| inner.read_char_converting(BlockingMode::NonBlocking))
            .is_some()
        {}
    }
}

impl console::interface::Statistics for PL011Uart {
    fn chars_written(&self) -> usize {
        self.inner.lock(|inner| inner.chars_written)
    }

    fn chars_read(&self) -> usize {
        self.inner.lock(|inner| inner.chars_read)
    }
}

use crate::{bsp, memory, time};

impl console::interface::All for PL011Uart {}

impl exception::asynchronous::interface::IRQHandler for PL011Uart {
    fn handle(&self) -> Result<(), &'static str> {
        self.inner.lock(|inner| {
            let pending = inner.registers.MIS.extract();

            // Clear all pending IRQs.
            inner.registers.ICR.write(ICR::ALL::CLEAR);

            // Check for any kind of RX interrupt.
            if pending.matches_any(MIS::RXMIS::SET + MIS::RTMIS::SET) {
                // Echo any received characters.
                while let Some(c) = inner.read_char_converting(BlockingMode::NonBlocking) {
                    inner.write_char(c);

                    match c {
                        '\n' => {
                            // Process the command
                            let command = core::str::from_utf8(&inner.cmd_buf[..inner.cmd_len])
                                .unwrap_or("")
                                .trim();

                            // Privilege level
                            if command.starts_with("level") {
                                let (_, privilege_level) = exception::current_privilege_level();
                                info!("Current privilege level: {}", privilege_level);
                            }
                            // GPIO RESET
                            else if command.starts_with("reset_gpio") {
                                info!("Reset All GPIO Connections");
                                stop_all_patterns();
                                reset_gpio();
                            }
                            // GPIO ON
                            else if command.starts_with("gpio_on") {
                                let parts: Vec<&str> = command.split_whitespace().collect();
                                info!("{:?}", parts);
                                gpio_on(parts[1].parse::<i32>().unwrap() as u8);
                                info!("{} on", parts[1]);
                            }
                            // GPIO OFF
                            else if command.starts_with("gpio_off") {
                                let parts: Vec<&str> = command.split_whitespace().collect();
                                info!("{:?}", parts[1]);
                                gpio_off(parts[1].parse::<i32>().unwrap() as u8);
                                info!("{} off", parts[1]);
                            }
                            // Board Name
                            else if command.starts_with("board_name") {
                                info!("Booting on: {}", bsp::board_name());
                            }
                            // Timer Resolution
                            else if command.starts_with("timer_resolution") {
                                info!(
                                    "Architectural timer resolution: {} ns",
                                    time::time_manager().resolution().as_nanos()
                                );
                            }
                            // MMU
                            else if command.starts_with("mmu") {
                                info!("MMU online:");
                                memory::mmu::kernel_print_mappings();
                            }
                            // Driver
                            else if command.starts_with("driver") {
                                info!("Drivers loaded:");
                                driver::driver_manager().enumerate();
                            }
                            // Driver
                            else if command.starts_with("irq_handler") {
                                info!("Registered IRQ handlers:");
                                exception::asynchronous::irq_manager().print_handler();
                            }
                            // Kernel Heap
                            else if command.starts_with("kernel_heap") {
                                info!("Kernel heap:");
                                memory::heap_alloc::kernel_heap_allocator().print_usage();
                            }
                            // Hex Counter
                            else if command.starts_with("hex_counter") {
                                stop_all_patterns();
                                unsafe {
                                    HEX_RUNNING = true;
                                    CURRENT_PATTERN = Some(PatternType::Hex);
                                }
                                info!("Hex Counter:");
                                start_hex_counter();
                            }
                            // Left Counter
                            else if command.starts_with("left_counter") {
                                stop_all_patterns();
                                unsafe {
                                    LEFT_RUNNING = true;
                                    CURRENT_PATTERN = Some(PatternType::Left);
                                }
                                info!("Left Counter:");
                                start_left_ring_counter();
                            }
                            // Right Counter
                            else if command.starts_with("right_counter") {
                                stop_all_patterns();
                                unsafe {
                                    RIGHT_RUNNING = true;
                                    CURRENT_PATTERN = Some(PatternType::Right);
                                }
                                info!("Right Counter:");
                                start_right_ring_counter();
                            }
                            // Dhrystone
                            else if command.starts_with("test") {
                                run_dhrystone();
                            }
                            // Not found
                            else {
                                info!("Command not found: ");
                            }

                            inner.cmd_len = 0;
                        }

                        _ => {
                            if inner.cmd_len < inner.cmd_buf.len() {
                                inner.cmd_buf[inner.cmd_len] = c as u8;
                                inner.cmd_len += 1;
                            } else {
                                // Command too long, reset and notify
                                inner.cmd_len = 0;
                                for b in b"Command too long\n" {
                                    inner.write_char(*b as char);
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }
}

fn reset_gpio() {
    for pinNumber in RING_PINS {
        setup_output(pinNumber);
        gpio_off(pinNumber);
    }
}

// Programs
fn gpio_on(pin: u8) {
    setup_output(pin);
    unsafe { bsp::driver::gpio_high(pin) };
    // info!("{} on", pin);
}
fn gpio_off(pin: u8) {
    setup_output(pin);
    unsafe { bsp::driver::gpio_low(pin) };
    // info!("{} off", pin);
}

fn gpio_on_after(pin: u8, seconds: u64) {
    time::time_manager()
        .set_timeout_once(Duration::from_secs(seconds), Box::new(move || gpio_on(pin)));
}

fn gpio_off_after(pin: u8, seconds: u64) {
    time::time_manager().set_timeout_once(
        Duration::from_secs(seconds),
        Box::new(move || gpio_off(pin)),
    );
}

// Counters (Move to other file)

static mut HEX_RUNNING: bool = false;
static mut LEFT_RUNNING: bool = false;
static mut RIGHT_RUNNING: bool = false;

#[derive(PartialEq, Eq, Clone, Copy)]
enum PatternType {
    Hex,
    Left,
    Right,
}

static mut CURRENT_PATTERN: Option<PatternType> = None;

const HEX_PINS: [u8; 4] = [1, 2, 3, 4];
const RING_PINS: [u8; 5] = [1, 2, 3, 4, 5];

fn stop_all_patterns() {
    unsafe {
        HEX_RUNNING = false;
        LEFT_RUNNING = false;
        RIGHT_RUNNING = false;
        CURRENT_PATTERN = None;
    }
}

fn setup_output(pin: u8) {
    unsafe {
        bsp::driver::gpio_as_output(pin);
    }
}

fn hex_counter_step(step: u8) {
    unsafe {
        if !HEX_RUNNING {
            return;
        }
    }
    let value = step & 0x0F;

    for (i, &pin) in HEX_PINS.iter().enumerate() {
        setup_output(pin);
        if (value >> i) & 1 == 1 {
            gpio_on(pin);
        } else {
            gpio_off(pin);
        }
    }
    info!("----------------------");

    if (step + 1) == 16 {
        stop_all_patterns();
        reset_gpio();
        return;
    }

    // Schedule next step
    time::time_manager().set_timeout_once(
        Duration::from_secs(1),
        Box::new(move || hex_counter_step((step + 1) % 16)),
    );
}

fn start_hex_counter() {
    hex_counter_step(0);
}

fn left_ring_counter_step(index: usize) {
    unsafe {
        if !LEFT_RUNNING {
            return;
        }
    }
    for (i, &pin) in RING_PINS.iter().enumerate() {
        setup_output(pin);
        if i == index {
            gpio_on(pin);
        } else {
            gpio_off(pin);
        }
    }
    info!("----------------------");

    if (index + 1) == RING_PINS.len() {
        stop_all_patterns();
        reset_gpio();
        return;
    }

    // Schedule next step
    let next = (index + 1) % RING_PINS.len();
    time::time_manager().set_timeout_once(
        Duration::from_secs(1),
        Box::new(move || left_ring_counter_step(next)),
    );
}

fn start_left_ring_counter() {
    left_ring_counter_step(0);
}

fn right_ring_counter_step(index: usize) {
    unsafe {
        if !RIGHT_RUNNING {
            return;
        }
    }
    for (i, &pin) in RING_PINS.iter().enumerate() {
        setup_output(pin);
        if i == index {
            gpio_on(pin);
        } else {
            gpio_off(pin);
        }
    }
    info!("----------------------");
    // Schedule next step
    let next = if index == 0 {
        stop_all_patterns();
        reset_gpio();
        return;
    } else {
        index - 1
    };

    time::time_manager().set_timeout_once(
        Duration::from_secs(1),
        Box::new(move || right_ring_counter_step(next)),
    );
}

fn start_right_ring_counter() {
    right_ring_counter_step(RING_PINS.len() - 1);
}

#[repr(C)]
struct Record<'a> {
    ptr_comp: Option<&'a mut Record<'a>>,
    discr: i32,
    enum_comp: i32,
    int_comp: i32,
    string_comp: &'a str,
}

static STRING1: &str = "DHRYSTONE PROGRAM, 1'ST STRING";

pub fn run_dhrystone() {
    const ITERATIONS: usize = 10_000;

    // Create two records on stack
    let mut record1 = Record {
        ptr_comp: None,
        discr: 0,
        enum_comp: 0,
        int_comp: 0,
        string_comp: STRING1,
    };

    let mut record2 = Record {
        ptr_comp: None,
        discr: 0,
        enum_comp: 0,
        int_comp: 0,
        string_comp: STRING1,
    };

    record1.ptr_comp = Some(&mut record2);

    let mut int1 = 0;
    let mut int2 = 0;
    let mut int3 = 0;

    let mut char1 = 'A';
    let mut char2 = 'B';

    info!("Running {} Dhrystone iterations...", ITERATIONS);

    let start_cycles = get_cycle_count(); // You'll implement this
    for _ in 0..ITERATIONS {
        // Integer ops
        int1 = 2;
        int2 = 3;
        int3 = int1 + int2;

        // Conditional
        if char1 != char2 {
            int3 += 1;
        }

        // Struct manipulation
        if let Some(ptr) = record1.ptr_comp.as_mut() {
            ptr.int_comp = int3;
            ptr.string_comp = "DHRYSTONE STRING";
        }

        // Simulate some string ops
        let _ = &record1.string_comp[0..5];
    }
    let end_cycles = get_cycle_count();

    let total_cycles = end_cycles.wrapping_sub(start_cycles);
    let cycles_per_iter = total_cycles as f64 / ITERATIONS as f64;

    info!("Dhrystone done.");
    info!("Total cycles: {}", total_cycles);
    info!("Cycles per iteration: {:.2}", cycles_per_iter);
}

fn get_cycle_count() -> u64 {
    let value: u64;
    unsafe {
        asm!(
            "mrs {value}, cntvct_el0",
            value = out(reg) value
        );
    }
    value
}
