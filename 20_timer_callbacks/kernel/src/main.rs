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
