//! The `kernel` binary.

#![feature(format_args_nl)]
#![no_main]
#![no_std]

extern crate alloc;

use core::time::Duration;

use alloc::boxed::Box;
use libkernel::{bsp, cpu, driver, exception, info, memory, state, time};

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

    show_logo();
    reset_gpio();

    info!("Echoing input now");
    cpu::wait_forever();
}

fn show_logo() {
    info!("   ________________________________________________________  ");
    info!("  /________________________________________________________| ");
    info!(" | ##    ##  ######    ##     ##     ## ##       #######   | ");
    info!(" | ##   ##   ##   ##   ##     ##  ##       ##  ##          | ");
    info!(" | ##  ##    ##   ##   ##     ##  ##       ##  ##          | ");
    info!(" | ## #      #####     ## ### ##  ##       ##     ###      | ");
    info!(" | ##  ##    ##   ##   ##     ##  ##       ##        ###   | ");
    info!(" | ##   ##   ##    ##  ##     ##  ##       ##          ##  | ");
    info!(" | ##    ##  ##     ## ##     ##     ## ##      ########   | ");
    info!(" |_________________________________________________________| ");
    info!(" |________________________________________________________/  ");
    info!("     K         R          H           O            S         ");
    info!("------------------------v 0.1.0----------------------------- ");
}

fn reset_gpio() {
    for pinNumber in [1, 2, 3, 4, 5] {
        setup_output(pinNumber);
        gpio_off(pinNumber);
    }
}

fn gpio_off(pin: u8) {
    setup_output(pin);
    unsafe { bsp::driver::gpio_low(pin) };
    // info!("{} off", pin);
}

fn setup_output(pin: u8) {
    unsafe {
        bsp::driver::gpio_as_output(pin);
    }
}
