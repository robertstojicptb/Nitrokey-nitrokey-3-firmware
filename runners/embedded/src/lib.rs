#![no_std]

use interchange::Interchange;
use littlefs2::fs::Filesystem;
use soc::types::Soc as SocT;
use types::Soc;
use usb_device::device::{UsbDeviceBuilder, UsbVidPid};

extern crate delog;
delog::generate_macros!();

pub mod types;

#[cfg(not(any(feature = "soc-lpc55", feature = "soc-nrf52840")))]
compile_error!("No SoC chosen!");

#[cfg_attr(feature = "soc-nrf52840", path = "soc_nrf52840/mod.rs")]
#[cfg_attr(feature = "soc-lpc55", path = "soc_lpc55/mod.rs")]
pub mod soc;

pub fn banner() {
		info_now!("Embedded Runner ({}:{}) using librunner {}.{}.{}",
			<SocT as Soc>::SOC_NAME,
			<SocT as Soc>::BOARD_NAME,
			types::build_constants::CARGO_PKG_VERSION_MAJOR,
			types::build_constants::CARGO_PKG_VERSION_MINOR,
			types::build_constants::CARGO_PKG_VERSION_PATCH);
}

pub fn init_store(int_flash: <SocT as Soc>::InternalFlashStorage, ext_flash: <SocT as Soc>::ExternalFlashStorage) -> types::RunnerStore {
	unsafe {
		types::INTERNAL_STORAGE = Some(int_flash);
		types::EXTERNAL_STORAGE = Some(ext_flash);
		types::VOLATILE_STORAGE = Some(types::VolatileStorage::new());

		types::INTERNAL_FS_ALLOC = Some(Filesystem::allocate());
		types::EXTERNAL_FS_ALLOC = Some(Filesystem::allocate());
		types::VOLATILE_FS_ALLOC = Some(Filesystem::allocate());
	}

	let store = types::RunnerStore::claim().unwrap();

	store.mount(
		unsafe { types::INTERNAL_FS_ALLOC.as_mut().unwrap() },
		unsafe { types::INTERNAL_STORAGE.as_mut().unwrap() },
		unsafe { types::EXTERNAL_FS_ALLOC.as_mut().unwrap() },
		unsafe { types::EXTERNAL_STORAGE.as_mut().unwrap() },
		unsafe { types::VOLATILE_FS_ALLOC.as_mut().unwrap() },
		unsafe { types::VOLATILE_STORAGE.as_mut().unwrap() },
		true
        ).expect("store.mount() error");

	store
}

pub fn init_usb_nfc(usbbus_opt: Option<&'static usb_device::bus::UsbBusAllocator<<SocT as Soc>::UsbBus>>,
		_nfc_opt: Option<nfc_device::Iso14443<<SocT as Soc>::NfcDevice>>) -> types::usb::UsbInit {

	/* claim interchanges */
	let (ccid_rq, ccid_rp) = apdu_dispatch::interchanges::Contact::claim().unwrap();
	let (nfc_rq, nfc_rp) = apdu_dispatch::interchanges::Contactless::claim().unwrap();
	let (ctaphid_rq, ctaphid_rp) = ctaphid_dispatch::types::HidInterchange::claim().unwrap();

	/* initialize dispatchers */
	let apdu_dispatch = apdu_dispatch::dispatch::ApduDispatch::new(ccid_rp, nfc_rp);
	let ctaphid_dispatch = ctaphid_dispatch::dispatch::Dispatch::new(ctaphid_rp);

	/* populate requesters (if bus options are provided) */
	let mut usb_classes = None;

	if let Some(usbbus) = usbbus_opt {
		/* Class #1: CCID */
		let ccid = usbd_ccid::Ccid::new(usbbus, ccid_rq, Some(b"PTB/EMC"));

		/* Class #2: CTAPHID */
		let ctaphid = usbd_ctaphid::CtapHid::new(usbbus, ctaphid_rq, 0u32)
			.implements_ctap1()
			.implements_ctap2()
			.implements_wink();

		/* Class #3: Serial */
		let serial = usbd_serial::SerialPort::new(usbbus);

		let usbdev = UsbDeviceBuilder::new(usbbus, UsbVidPid(0x1209, 0x5090))
			.product("EMC Stick")
			.manufacturer("Nitrokey/PTB")
			.serial_number("imagine.a.uuid.here")
			.device_release(0x0001u16)
			.max_packet_size_0(64)
			.composite_with_iads()
			.build();

		usb_classes = Some(types::usb::UsbClasses::new(usbdev, ccid, ctaphid, serial));
	}

	types::usb::UsbInit { usb_classes, apdu_dispatch, ctaphid_dispatch }
}

#[cfg(feature = "provisioner-app")]
pub fn init_apps(trussed: &mut types::Trussed, store: &types::RunnerStore, on_nfc_power: bool) -> types::Apps {
	let store_2 = store.clone();
	let int_flash_ref = unsafe { types::INTERNAL_STORAGE.as_mut().unwrap() };
	let pnp = types::ProvisionerNonPortable {
		store: store_2,
		stolen_filesystem: int_flash_ref,
		nfc_powered: on_nfc_power
	};
	types::Apps::new(trussed, pnp)
}

#[cfg(not(feature = "provisioner-app"))]
pub fn init_apps(trussed: &mut types::Trussed, _store: &types::RunnerStore, _on_nfc_power: bool) -> types::Apps {
	types::Apps::new(trussed)
}
