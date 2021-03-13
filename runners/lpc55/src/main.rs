//! main app in cortex-m-rtic version
//!
//! See also `main_rt.rs` for a RT-only version.

#![no_std]
#![no_main]
// #![deny(warnings)]

use app::hal;
use hal::traits::wg::timer::Cancel;
use hal::traits::wg::timer::CountDown;
use hal::drivers::timer::Lap;
use hal::time::*;

use rtic::cyccnt::{Instant, U32Ext as _};

const CLOCK_FREQ: u32 = 96_000_000;
const PERIOD: u32 = CLOCK_FREQ/16;

#[macro_use]
extern crate delog;
generate_macros!();

#[rtic::app(device = app::hal::raw, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {

    struct Resources {
        apdu_dispatch: app::types::ApduDispatch,
        hid_dispatch: app::types::HidDispatch,
        trussed: app::types::Trussed,

        piv: app::types::Piv,
        fido: app::types::FidoApplet<fido_authenticator::NonSilentAuthenticator>,
        ndef: applet_ndef::NdefApplet<'static>,
        root: app::types::RootApp,

        usb_classes: Option<app::types::UsbClasses>,
        contactless: Option<app::types::Iso14443>,

        perf_timer: app::types::PerfTimer,

        clock_ctrl: Option<app::types::DynamicClockController>,
        hw_scheduler: app::types::HwScheduler,
    }

    #[init(schedule = [update_ui])]
    fn init(c: init::Context) -> init::LateResources {

        let (
            apdu_dispatch,
            hid_dispatch,
            trussed,

            piv,
            fido,
            ndef,
            root,

            usb_classes,
            contactless,

            perf_timer,
            clock_ctrl,
            hw_scheduler,
        ) = app::init_board(c.device, c.core);

        // don't toggle LED in passive mode
        if usb_classes.is_some() {
            hal::enable_cycle_counter();
            c.schedule.update_ui(Instant::now() + PERIOD.cycles()).unwrap();
        }

        init::LateResources {
            apdu_dispatch,
            hid_dispatch,
            trussed,

            piv,
            fido,
            ndef,
            root,

            usb_classes,
            contactless,

            perf_timer,

            clock_ctrl,
            hw_scheduler,
        }
    }

    #[idle(resources = [usb_classes, apdu_dispatch, hid_dispatch, ndef, piv, fido, root, contactless, perf_timer])]
    fn idle(c: idle::Context) -> ! {
        let idle::Resources {
            mut usb_classes,
            apdu_dispatch,
            hid_dispatch,
            ndef,
            piv,
            fido,
            root,
            mut contactless,
            mut perf_timer,
        }
            = c.resources;

        info_now!("inside IDLE");
        loop {

            let mut time = 0;
            perf_timer.lock(|perf_timer|{
                time = perf_timer.lap().0;
                if time == 60_000_000 {
                    perf_timer.start(60_000.ms());
                }
            });
            if time > 1_000_000 {
                app::Delogger::flush();
            }

            apdu_dispatch.poll(&mut [ndef, piv, fido, root]);

            contactless.lock(|contactless|  {
                match contactless.as_ref() {
                    Some(contactless) => {
                        if contactless.is_ready_to_transmit() {
                            rtic::pend(board::hal::raw::Interrupt::PIN_INT0);
                        }
                    }
                    _ => {}
                }
            });

            usb_classes.lock(|usb_classes_maybe| {
                match usb_classes_maybe.as_mut() {
                    Some(usb_classes) => {
                        usb_classes.ctaphid.check_for_responses();
                        // the `usbd.poll` only calls its classes if
                        // there is activity on the bus. hence we need
                        // to kick ccid to pick up responses...
                        usb_classes.ccid.sneaky_poll();
                        usb_classes.usbd.poll(&mut [
                            &mut usb_classes.ccid,
                            &mut usb_classes.ctaphid,
                            &mut usb_classes.serial
                        ]);
                        usb_classes.ctaphid.check_timeout(time/1000);
                    }
                    _ => {}
                }
            } );


            hid_dispatch.poll(&mut [fido, root]);

        }
    }

    #[task(binds = USB1_NEEDCLK, resources = [usb_classes], priority=6)]
    fn usb1_needclk(c: usb1_needclk::Context) {
        let usb_classes = c.resources.usb_classes.as_mut().unwrap();
        usb_classes.usbd.poll(&mut [&mut usb_classes.ccid, &mut usb_classes.ctaphid, &mut usb_classes.serial]);
    }

    #[task(binds = USB1, resources = [usb_classes], priority=6)]
    fn usb1(c: usb1::Context) {
        let usb = unsafe { hal::raw::Peripherals::steal().USB1 } ;
        let before = Instant::now();
        let usb_classes = c.resources.usb_classes.as_mut().unwrap();

        //////////////
        usb_classes.usbd.poll(&mut [&mut usb_classes.ccid, &mut usb_classes.ctaphid, &mut usb_classes.serial]);
        //////////////

        let after = Instant::now();
        let length = (after - before).as_cycles();
        if length > 10_000 {
            info!("poll took {:?} cycles", length);
        }
        let inten = usb.inten.read().bits();
        let intstat = usb.intstat.read().bits();
        let mask = inten & intstat;
        if mask != 0 {
            info!("uncleared interrupts: {:?}", mask);
            for i in 0..5 {
                if mask & (1 << 2*i) != 0 {
                    info!("EP{}OUT", i);
                }
                if mask & (1 << (2*i + 1)) != 0 {
                    info!("EP{}IN", i);
                }
            }
            // Serial sends a stray 0x70 ("p") to CDC-ACM "data" OUT endpoint (3)
            // Need to fix that at the root, for now just clear that interrupt.
            usb.intstat.write(|w| unsafe{ w.bits(64) });
            // usb.intstat.write(|w| unsafe{ w.bits( usb.intstat.read().bits() ) });
        }

    }

    #[task(binds = MAILBOX, resources = [usb_classes], priority = 5)]
    #[allow(unused_mut,unused_variables)]
    fn mailbox(mut c: mailbox::Context) {
        #[cfg(feature = "log-serial")]
        c.resources.usb_classes.lock(|usb_classes_maybe| {
            match usb_classes_maybe.as_mut() {
                Some(usb_classes) => {
                    // usb_classes.serial.write(logs.as_bytes()).ok();
                    usb_classes.serial.write(b"dummy test string\n").ok();
                    // app::drain_log_to_serial(&mut usb_classes.serial);
                }
                _=>{}
            }
        });
        // // let usb_classes = c.resources.usb_classes.as_mut().unwrap();

        // let mailbox::Resources { usb_classes } = c.resources;
        // let x: () = usb_classes;
        // // if let Some(usb_classes) = usb_classes.as_mut() {
        // //     usb_classes.serial.write(b"dummy test string\n").ok();
        // // }
    }

    #[task(binds = OS_EVENT, resources = [trussed], priority = 5)]
    fn os_event(c: os_event::Context) {
        c.resources.trussed.process();
    }

    #[task(resources = [trussed], schedule = [update_ui], priority = 1)]
    fn update_ui(mut c: update_ui::Context) {

        static mut UPDATES: u32 = 1;

        // let wait_periods = c.resources.trussed.lock(|trussed| trussed.update_ui());
        c.resources.trussed.lock(|trussed| trussed.update_ui());
        // c.schedule.update_ui(Instant::now() + wait_periods * PERIOD.cycles()).unwrap();
        c.schedule.update_ui(Instant::now() + PERIOD.cycles()).unwrap();

        *UPDATES += 1;
    }

    #[task(binds = CTIMER0, resources = [contactless, perf_timer, hw_scheduler], priority = 7)]
    fn nfc_wait_extension(c: nfc_wait_extension::Context) {
        let nfc_wait_extension::Resources {
            contactless,
            perf_timer: _perf_timer,
            hw_scheduler,
        }
            = c.resources;
        if let Some(contactless) = contactless.as_mut() {

            // clear the interrupt
            hw_scheduler.cancel().ok();

            info!("<{}", _perf_timer.lap().0/100);
            let status = contactless.poll_wait_extensions();
            match status {
                iso14443::Iso14443Status::Idle => {}
                iso14443::Iso14443Status::ReceivedData(duration) => {
                    hw_scheduler.start(duration.subsec_millis().ms());
                }
            }
            info!(" {}>", _perf_timer.lap().0/100);
        }
    }

    #[task(binds = PIN_INT0, resources = [
            contactless, perf_timer, hw_scheduler,
        ], priority = 7,
    )]
    fn nfc_irq(c: nfc_irq::Context) {

        let nfc_irq::Resources {
            contactless,
            perf_timer,
            hw_scheduler,
            }
            = c.resources;
        let contactless = contactless.as_mut().unwrap();
        let _starttime = perf_timer.lap().0/100;

        info!("[");
        let status = contactless.poll();
        match status {
            iso14443::Iso14443Status::Idle => {}
            iso14443::Iso14443Status::ReceivedData(duration) => {
                hw_scheduler.cancel().ok();
                hw_scheduler.start(duration.subsec_millis().ms());
            }
        }
        info!("{}-{}]", _starttime, perf_timer.lap().0/100);

        perf_timer.cancel().ok();
        perf_timer.start(60_000.ms());
    }

    #[task(binds = ADC0, resources = [clock_ctrl], priority = 8)]
    fn adc_int(c: adc_int::Context) {
        let adc_int::Resources {
            clock_ctrl,
        } = c.resources;
        clock_ctrl.as_mut().unwrap().handle();
    }


    // something to dispatch software tasks from
    extern "C" {
        fn PLU();
        fn PIN_INT7();
    }

};