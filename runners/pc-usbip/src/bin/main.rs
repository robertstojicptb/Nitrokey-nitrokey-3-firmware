use std::cell::RefCell;
use std::rc::Rc;

use clap::Parser;
use clap_num::maybe_hex;

use interchange::Interchange;
use usb_device::{bus::UsbBusAllocator, prelude::*};
use usbip_device::UsbIpBus;

use solo_usbip::platform::{Platform, init_platform};
use fido_authenticator;
use usbd_ctaphid::constants::MESSAGE_SIZE;

pub type FidoConfig = fido_authenticator::Config;

/// USP/IP based virtualization of the Nitrokey 3 / Solo2 device.
/// Supports FIDO application at the moment.
#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    /// USB Name string
    #[clap(short, long, default_value = "FIDO authenticator")]
    name: String,

    /// USB Manufacturer string
    #[clap(short, long, default_value = "Simulation")]
    manufacturer: String,

    /// USB Serial string
    #[clap(long, default_value = "SIM SIM SIM")]
    serial: String,

    /// Trussed state file
    #[clap(long, default_value = "trussed-state.bin")]
    state_file: String,

    /// USB VID id
    #[clap(short, long, parse(try_from_str=maybe_hex), default_value_t = 0x20a0)]
    vid: u16,
    /// USB PID id
    #[clap(short, long, parse(try_from_str=maybe_hex), default_value_t = 0x42b2)]
    pid: u16,
}

struct Reboot;

impl admin_app::Reboot for Reboot {
    fn reboot() -> ! {
        unimplemented!();
    }

    fn reboot_to_firmware_update() -> ! {
        unimplemented!();
    }

    fn reboot_to_firmware_update_destructive() -> ! {
        unimplemented!();
    }

    fn locked() -> bool {
        false
    }
}

#[derive(Clone)]
struct Syscall {
    service: Rc<RefCell<trussed::service::Service<Platform>>>,
}

impl trussed::client::Syscall for Syscall {
    fn syscall(&mut self) {
        log::debug!("syscall");
        self.service.borrow_mut().process();
    }
}

fn main() {
    #[cfg(feature = "enable-logs")]
    pretty_env_logger::init();
    let args = Args::parse();

    log::info!("Initializing Trussed");
    let state_file = args.state_file;
    let trussed_platform = init_platform(state_file);
    let trussed_service = Rc::new(RefCell::new(trussed::service::Service::new(trussed_platform)));

    log::info!("Initializing allocator");
    // To change IP or port see usbip-device-0.1.4/src/handler.rs:26
    let bus_allocator = UsbBusAllocator::new(UsbIpBus::new());
    let (ctaphid_rq, ctaphid_rp) = ctaphid_dispatch::types::HidInterchange::claim().unwrap();
    let mut ctaphid = usbd_ctaphid::CtapHid::new(&bus_allocator, ctaphid_rq, 0u32)
        .implements_ctap1()
        .implements_ctap2()
        .implements_wink();
    let mut ctaphid_dispatch = ctaphid_dispatch::dispatch::Dispatch::new(ctaphid_rp);
    let mut usb_bus = UsbDeviceBuilder::new(&bus_allocator, UsbVidPid(args.vid, args.pid))
        .manufacturer(&args.manufacturer)
        .product(&args.name)
        .serial_number(&args.serial)
        .device_class(0x03)
        .device_sub_class(0)
        .build();

    let syscall = Syscall { service: trussed_service.clone() };

    let trussed_client = trussed_service.borrow_mut().try_new_client("fido", syscall.clone()).unwrap();
    let mut fido_app = fido_authenticator::Authenticator::new(
        trussed_client,
        fido_authenticator::Conforming {},
        fido_authenticator::Config { max_msg_size: MESSAGE_SIZE},
    );

    let trussed_client = trussed_service.borrow_mut().try_new_client("admin", syscall.clone()).unwrap();
    let mut admin_app = admin_app::App::<_, Reboot>::new(trussed_client, [0; 16], 0);

    log::info!("Ready for work");
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        ctaphid_dispatch.poll(&mut [&mut fido_app, &mut admin_app]);
        usb_bus.poll(&mut [&mut ctaphid]);
    }
}