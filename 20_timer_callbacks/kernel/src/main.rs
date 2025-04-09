// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2023 Andre Richter <andre.o.richter@gmail.com>

// Rust embedded logo for `make doc`.
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/rust-embedded/wg/master/assets/logo/ewg-logo-blue-white-on-transparent.png"
)]

//! The `kernel` binary.

#![feature(format_args_nl)]
#![no_main]
#![no_std]

extern crate alloc;

use core::time::Duration;

use alloc::boxed::Box;
use libkernel::{bsp, cpu, driver, exception, info, memory, state, time};

/// Early init code.
///
/// When this code runs, virtual memory is already enabled.
///
/// # Safety
///
/// - Only a single core must be active and running this function.
/// - Printing will not work until the respective driver's MMIO is remapped.
#[no_mangle]
unsafe fn kernel_init() -> ! {
    exception::handling_init();
    memory::init();

    // Initialize the timer subsystem.
    if let Err(x) = time::init() {
        panic!("Error initializing timer subsystem: {}", x);
    }

    // Initialize the BSP driver subsystem.
    if let Err(x) = bsp::driver::init() {
        panic!("Error initializing BSP driver subsystem: {}", x);
    }

    // Initialize all device drivers.
    driver::driver_manager().init_drivers_and_irqs();

    bsp::memory::mmu::kernel_add_mapping_records_for_precomputed();

    // Unmask interrupts on the boot CPU core.
    exception::asynchronous::local_irq_unmask();

    // Announce conclusion of the kernel_init() phase.
    state::state_manager().transition_to_single_core_main();

    // Transition from unsafe to safe.
    kernel_main()
}

/// The main function running after the early init.
fn kernel_main() -> ! {
    use alloc::boxed::Box;
    use core::time::Duration;

    // info!("{}", libkernel::version());

    // TODO: Seach what this does
    // info!("Exception handling state:");
    // exception::asynchronous::print_state();

    // time::time_manager().set_timeout_once(Duration::from_secs(5), Box::new(|| info!("Once 5")));
    // time::time_manager().set_timeout_once(Duration::from_secs(3), Box::new(|| info!("Once 2")));
    // time::time_manager()
    //     .set_timeout_periodic(Duration::from_secs(1), Box::new(|| info!("Periodic 1 sec")));

    // Dont use 14 or 15 (UART)
    let pin: u8 = 18;
    info!("GPIO Testing on PIN: {}", pin);
    unsafe {
        bsp::driver::gpio_as_output(pin);
    }

    // Counters
    info!("Hex Counter:");
    start_hex_counter();
    info!(" ");
    // info!("Left Ring Counter:");
    // start_left_ring_counter();
    // info!("Right Ring Counter:");
    // right_ring_counter_schedule();
    // info!(" ");

    // for i in (0..20).step_by(2) {
    //     time::time_manager()
    //         .set_timeout_once(Duration::from_secs(i), Box::new(move || gpio_on(pin)));
    //     time::time_manager()
    //         .set_timeout_once(Duration::from_secs(i + 1), Box::new(move || gpio_off(pin)));
    // }

    info!("Echoing input now");
    cpu::wait_forever();

    // After timer
    // use alloc::sync::Arc;
    // use core::sync::atomic::{AtomicBool, Ordering};
    //
    // let flag = Arc::new(AtomicBool::new(false));
    // let flag_clone = flag.clone();
    //
    // time_manager().set_timeout_once(
    //     Duration::from_secs(2),
    //     Box::new(move || {
    //         flag_clone.store(true, Ordering::Relaxed);
    //     }),
    // );
    //
    // // Later in your code (maybe in a loop or another IRQ)
    // if flag.load(Ordering::Relaxed) {
    //     println!("✅ Timer has finished!");
    // }
}
fn gpio_on(pin: u8) {
    unsafe { bsp::driver::gpio_high(pin) };
    info!("{} on", pin);
}
fn gpio_off(pin: u8) {
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

const HEX_PINS: [u8; 4] = [1, 2, 3, 4];
const RING_PINS: [u8; 5] = [5, 6, 7, 8, 9];

fn setup_output(pin: u8) {
    unsafe {
        bsp::driver::gpio_as_output(pin);
    }
}

fn hex_counter_step(step: u8) {
    let value = step & 0x0F;

    for (i, &pin) in HEX_PINS.iter().enumerate() {
        setup_output(pin);
        if (value >> i) & 1 == 1 {
            gpio_on(pin);
        } else {
            gpio_off(pin);
        }
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
    for (i, &pin) in RING_PINS.iter().enumerate() {
        setup_output(pin);
        if i == index {
            gpio_on(pin);
        } else {
            gpio_off(pin);
        }
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
    for (i, &pin) in RING_PINS.iter().enumerate() {
        setup_output(pin);
        if i == index {
            gpio_on(pin);
        } else {
            gpio_off(pin);
        }
    }

    // Schedule next step
    let next = if index == 0 {
        RING_PINS.len() - 1
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
