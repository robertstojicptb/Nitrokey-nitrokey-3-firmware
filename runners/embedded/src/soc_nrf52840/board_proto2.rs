use super::display_ui::ButtonPin::*;
use super::types::BoardGPIO;
use embedded_hal::blocking::delay::DelayUs;
use nrf52840_hal::{
    self as hal,//ERWEITERT
    gpio::{p0, p1, Input, Level, Output, Pin, PullUp, PushPull},
    gpiote::Gpiote,
    spim,
    pac::TIMER0,//ERWEITERT
    prelude::*,//ERWEITERT
    timer::OneShot,//ERWEITERT
    Temp,    //ERWEITERT
    Timer,//ERWEITERT
};

use cortex_m::asm::delay; //ERWEITERT

use embedded_hal::prelude::*; //ERWEITERT

use asm_delay::AsmDelay; //ERWEITERT
use asm_delay::bitrate::*; //ERWEITERT



pub type InPin = Pin<Input<PullUp>>;
pub type OutPin = Pin<Output<PushPull>>;

pub type TrussedUI = super::display_ui::DisplayUI;

pub const BOARD_NAME: &'static str = "Proto2";
pub const KEEPALIVE_PINS: &'static [u8] = &[0x2a, 0x2b, 0x0e, 0x21];

pub fn init_ui(
    spi_pac: nrf52840_pac::SPIM0,
    spi_pins: spim::Pins,
    d_dc: OutPin,
    d_reset: OutPin,
    d_power: Option<OutPin>,
    d_backlight: Option<OutPin>,
    mut buttons: [Option<InPin>; 8],
    leds: [Option<OutPin>; 4],
    delay_timer: &mut impl DelayUs<u32>,
) -> TrussedUI {
    let spim0 = nrf52840_hal::spim::Spim::new(
        spi_pac,
        spi_pins,
        nrf52840_hal::spim::Frequency::M8,
        nrf52840_hal::spim::MODE_3,
        0x7e_u8,
    );

    let disp_intf = display_interface_spi::SPIInterfaceNoCS::new(spim0, d_dc);

    let disp_st7789 = picolcd114::ST7789::new(disp_intf, d_reset, 240, 135, 40, 53);

    let ui_buttons = [
        LowTriggerPin(buttons[0].take().unwrap()),
        LowTriggerPin(buttons[1].take().unwrap()),
        LowTriggerPin(buttons[2].take().unwrap()),
        LowTriggerPin(buttons[3].take().unwrap()),
        NoPin,
        NoPin,
        NoPin,
        NoPin,
    ];

    let mut ui = super::display_ui::DisplayUI::new(
        Some(disp_st7789),
        d_power,
        d_backlight,
        ui_buttons,
        leds,
        None,
    );
    ui.power_on(delay_timer);

    ui
}

pub fn init_pins(gpiote: &Gpiote, gpio_p0: p0::Parts, gpio_p1: p1::Parts) -> BoardGPIO {
    /* Buttons */
    let btn1 = gpio_p1.p1_10.into_pullup_input().degrade();
    let btn2 = gpio_p1.p1_11.into_pullup_input().degrade();
    let btn3 = gpio_p0.p0_14.into_pullup_input().degrade();
    let btn4 = gpio_p1.p1_01.into_pullup_input().degrade();

    gpiote
        .channel0()
        .input_pin(&btn1)
        .toggle()
        .enable_interrupt();
    gpiote
        .channel1()
        .input_pin(&btn2)
        .toggle()
        .enable_interrupt();
    gpiote
        .channel2()
        .input_pin(&btn3)
        .toggle()
        .enable_interrupt();
    gpiote
        .channel3()
        .input_pin(&btn4)
        .toggle()
        .enable_interrupt();

    /* Display SPI Bus */
    let dsp_spi_cs = gpio_p0.p0_06.into_push_pull_output(Level::Low).degrade();
    let dsp_spi_clk = gpio_p0.p0_01.into_push_pull_output(Level::Low).degrade();
    /* no MISO, unidirectional SPI */
    let dsp_spi_mosi = gpio_p0.p0_00.into_push_pull_output(Level::Low).degrade();
    let dsp_rst = gpio_p0.p0_04.into_push_pull_output(Level::Low).degrade();
    let dsp_dc = gpio_p0.p0_26.into_push_pull_output(Level::Low).degrade();
    let dsp_bl = gpio_p0.p0_08.into_push_pull_output(Level::Low).degrade();
    let dsp_pwr = gpio_p0.p0_13.into_push_pull_output(Level::High).degrade();

    let dsp_spi = spim::Pins {
        sck: dsp_spi_clk,
        miso: None,
        mosi: Some(dsp_spi_mosi),
    };

    /* Fingerprint */
    let fp_tx = gpio_p0.p0_12.into_push_pull_output(Level::Low).degrade();
    let fp_rx = gpio_p0.p0_11.into_floating_input().degrade();
    let fp_detect = gpio_p1.p1_09.into_pulldown_input().degrade();
    let fp_pwr = gpio_p0.p0_15.into_push_pull_output(Level::High).degrade();

    let uart_pins = nrf52840_hal::uarte::Pins {
        txd: fp_tx,
        rxd: fp_rx,
        cts: None,
        rts: None,
    };

    gpiote
        .channel4()
        .input_pin(&fp_detect)
        .lo_to_hi()
        .enable_interrupt();

    /* SE050 */
    let se_pwr = gpio_p0.p0_20.into_push_pull_output(Level::Low).degrade();
    let se_scl = gpio_p0.p0_22.into_floating_input().degrade();
    let se_sda = gpio_p0.p0_24.into_floating_input().degrade();

    let se_pins = nrf52840_hal::twim::Pins {
        scl: se_scl,
        sda: se_sda,
    };

    /* Flash & NFC SPI Bus */
    let flash_spi_cs = gpio_p0.p0_25.into_push_pull_output(Level::High).degrade();
    let flashnfc_spi_clk = gpio_p1.p1_02.into_push_pull_output(Level::Low).degrade();
    let flashnfc_spi_miso = gpio_p1.p1_06.into_floating_input().degrade();
    let flashnfc_spi_mosi = gpio_p1.p1_04.into_push_pull_output(Level::Low).degrade();
    
    let flash_power = gpio_p1.p1_00.into_push_pull_output(Level::Low).degrade();//ERWEITERT NEU

    let flashnfc_spi = spim::Pins {
        sck: flashnfc_spi_clk,
        miso: Some(flashnfc_spi_miso),
        mosi: Some(flashnfc_spi_mosi),        
    };

    BoardGPIO {
        buttons: [
            Some(btn1),
            Some(btn2),
            Some(btn3),
            Some(btn4),
            None,
            None,
            None,
            None,
        ],
        leds: [None, None, None, None],
        rgb_led: [None, None, None],
        touch: None,
        uart_pins: Some(uart_pins),
        fpr_detect: Some(fp_detect),
        fpr_power: Some(fp_pwr),
        display_spi: Some(dsp_spi),
        display_cs: Some(dsp_spi_cs),
        display_reset: Some(dsp_rst),
        display_dc: Some(dsp_dc),
        display_backlight: Some(dsp_bl),
        display_power: Some(dsp_pwr),
        se_pins: Some(se_pins),
        se_power: Some(se_pwr),
        flashnfc_spi: Some(flashnfc_spi),
        flash_cs: Some(flash_spi_cs),
        //flash_power: None, //ERWEITERT AUSKOMMENTIERT
        flash_power: Some(flash_power),
        nfc_cs: None,
        nfc_irq: None,
    }
}

pub fn gpio_irq_sources(dir: &[u32]) -> u32 {
    let mut src: u32 = 0;
    fn bit_set(x: u32, y: u32) -> bool {
        (x & (1u32 << y)) != 0
    }

    if !bit_set(dir[1], 10) {
        src |= 0b0000_0001;
    }
    if !bit_set(dir[1], 11) {
        src |= 0b0000_0010;
    }
    if !bit_set(dir[0], 14) {
        src |= 0b0000_0100;
    }
    if !bit_set(dir[1], 1) {
        src |= 0b0000_1000;
    }
    // if  bit_set(dir[x],  y) { src |= 0b1_0000_0000; }
    src
}

pub fn set_panic_led() {
    unsafe {
        let pac = nrf52840_pac::Peripherals::steal();
        let p0 = nrf52840_hal::gpio::p0::Parts::new(pac.P0);
        let p1 = nrf52840_hal::gpio::p1::Parts::new(pac.P1);

        // red
        p0.p0_09.into_push_pull_output(Level::Low).degrade();
        // green
        p0.p0_10.into_push_pull_output(Level::High).degrade();
        // blue
        p1.p1_02.into_push_pull_output(Level::High).degrade();
    }
}


//#######################################################################################################################
//Konfiguration welche circa 9 uA liefert bei protoyp 2- CLEAN UP
//Normalbetrieb circa 30mA

pub fn power_off() { 

    //let d = AsmDelay::new(64.mhz());
    let mut d = AsmDelay::new(asm_delay::bitrate::U32BitrateExt::mhz(64));
    d.delay_ms(10u32);

    let pac = unsafe { nrf52840_pac::Peripherals::steal() };
   // delay(10_000_000); //ERWEITERT 
    d.delay_ms(10u32);

    //#####################################################
     // only go into System OFF when we have no USB VBUS    

    if pac.POWER.usbregstatus.read().vbusdetect().is_vbus_present() {
        return;
    }
    //  delay(10_000_000);
     d.delay_ms(10u32);


    //#####################################################
    // external flash -> Deep Power Down with QUAD SPI// SPI QSPI  
    //Deep Power-Down (DP) (B9H) GD25Q16C
    
    //flash_spi_cs -> p0.p0_25  auf 1 setzen
    unsafe {
        pac.P0.pin_cnf[25].write(|w| w.bits(1));
    }  
    //delay(10_000_000); 
    d.delay_ms(10u32);

    //flash_spi_cs -> p0.p0_25  auf 0setzen
    unsafe {
        pac.P0.pin_cnf[25].write(|w| w.bits(0));
    }  
    //delay(10_000_000); 
    d.delay_ms(10u32);

    //B9H senden 
    unsafe {

        pac.QSPI.events_ready.write(|w| w.bits(0));

        //delay(5_000_000);
        d.delay_ms(10u32);

        pac.QSPI.cinstrconf.write(|w| {
            w.opcode()
                .bits(0xb9)
                .length()
                ._1b()
                .wipwait()
                .clear_bit()
                .wren()
                .clear_bit()
                .lfen()
                .clear_bit()
                .lfstop()
                .clear_bit()
        });

        //  delay(10_000_000);
          d.delay_ms(10u32);
        /*         
        loop {
            let p = pac.QSPI.events_ready.read().bits();

            
            if p != 0 {
                break;
            }
        }*/
    
    }

    //delay(5_000_000);
    //d.delay_ms(10u32);
 
    //flash_spi_cs -> p0.p0_25 auf 1 sezen
    
    unsafe {
        pac.P0.pin_cnf[25].write(|w| w.bits(1));
    }  
    //delay(10_000_000); 
    d.delay_ms(10u32);


    //#####################################################
    //    /* Flash & NFC SPI Bus */ //PINS


    //ERWEITERT
    //######################################
    //  flashnfc_spi_clk   p1.p1_02  mit 1 setzen s.g.
    
    unsafe {
        pac.P1.pin_cnf[2].write(|w| w.bits(1));
    }  
    //delay(5_000_000);   
    d.delay_ms(10u32);
   
    
    //######################################
    //  flashnfc_spi_miso p1.p1_06
    
    unsafe {
            pac.P1.pin_cnf[6].write(|w| w.bits(1));
        }  
    //delay(5_000_000);    
    d.delay_ms(10u32);

    //######################################
    //flashnfc_spi_mosi -> p1.p1_04 
    
    unsafe {
            pac.P1.pin_cnf[4].write(|w| w.bits(1));
        }  

        //delay(5_000_000);      
        d.delay_ms(10u32);



    //#################################
    //   _flash_pwr P1.0 ->p1.p1_00
    
    unsafe {
        pac.P1.pin_cnf[0].write(|w| w.bits(0));
    }  

    //delay(10_000_000);    
    d.delay_ms(10u32);

 
    //#####################################################
    //#####################################################
    //DISPLAY 

    //######################################    
    // display  BACKLIGHT->P0.08 S.g.w. TRANSISTOR

    unsafe {
        pac.P0.pin_cnf[8].write(|w| w.bits(0));
    }  

    //delay(10_000_000);
    d.delay_ms(10u32);

    //###################################### 
    //DISPLAY POWER    
    // display Transistor P0.13 w
    
    unsafe {
        pac.P0.pin_cnf[13].write(|w| w.bits(0));
    }  

    //delay(10_000_000);
    d.delay_ms(10u32);

    //######################################      
    // display MOSI->P0.00 w
    
    unsafe {
        pac.P0.pin_cnf[0].write(|w| w.bits(0));
    }  

    //delay(5_000_000);
    d.delay_ms(10u32);

    //######################################      
    // display  CLK->P0.01 W 
    
    unsafe {
        pac.P0.pin_cnf[1].write(|w| w.bits(0));
    }  

    //delay(5_000_000);
    d.delay_ms(10u32);
  
    //######################################      
    // display  CS->P0.06   W 

    unsafe 
    {
        pac.P0.outclr.write(|w| w.bits(1u32 << 6));
    } // CS

    //delay(5_000_000);
    d.delay_ms(10u32);

    //######################################      
    // display  DC->P0.26   W 

    unsafe {     
    pac.P0.pin_cnf[26].write(|w| w.bits(0));
    } 

    //delay(5_000_000);
    d.delay_ms(10u32);
    

    //######################################     
    //display  RESET->P0.04 W
    
    unsafe {
        pac.P0.pin_cnf[4].write(|w| w.bits(0));
    }  

    //delay(5_000_000);
    d.delay_ms(10u32);
 

    //##################################################################### 
    //###################################### 
    //SE050    

    //###################################### 
    // SE050 //I2C SCL ->P0.22 W
    
    unsafe {
    // pac.P0.pin_cnf[22].write(|w| w.bits(1));
        pac.P0.pin_cnf[22].write(|w| w.bits(1));
    } // SCL 

    //delay(15_000_000);
    d.delay_ms(10u32);

    //###################################### 
    // SE050 //I2C SDA ->P0.24  W
    
    unsafe {
    // pac.P0.pin_cnf[24].write(|w| w.bits(1));
        pac.P0.pin_cnf[24].write(|w| w.bits(1));
    } // SDA 

    //delay(15_000_000);
    d.delay_ms(10u32);


    //###################################### 
    // SE050 POWER ->P0.20  //W

    unsafe {
        pac.P0.pin_cnf[20].write(|w| w.bits(0));
    } 
    //delay(15_000_000);
    d.delay_ms(10u32);
        

    //##################################################################### 
    //###################################### 
    //fingerprint reader

    //######################################     
    // fingerprint reader //UART TX P0.12 W
    unsafe {
        pac.P0.pin_cnf[12].write(|w| w.bits(0));
    } // TX
    //delay(5_000_000);
    d.delay_ms(10u32);
    
    //######################################
    // fingerprint reader //UART RX P0.11 -W
    
    unsafe {
        pac.P0.pin_cnf[11].write(|w| w.bits(0));
    } // RX

    //delay(5_000_000);
    d.delay_ms(10u32);


    //###################################### 
    // fingerprint reader //UART DETECT/TRIGGER P1.09 W

    unsafe {
        pac.P1.pin_cnf[9].write(|w| w.bits(0));
    } // DETECT
    
    //delay(5_000_000);
    d.delay_ms(10u32);

    //###################################### 
    // fingerprint reader //UART PWR P0.15 W

    unsafe {
        pac.P0.pin_cnf[15].write(|w| w.bits(0));
    }

    //delay(5_000_000);    
    d.delay_ms(10u32);





   //##################################################################### 
    //#####################################################
    // NFCT, NVMC

    unsafe {
        //pac.CLOCK.tasks_hfclkstop.write(|w| w.bits(1));
        pac.CLOCK.tasks_hfclkstop.write(|w| w.bits(0));
    }
    //delay(15_000_000); 
    d.delay_ms(10u32);

   
 //#####################################################
    //UARTE0
    
    unsafe {
        pac.UARTE0.enable.write(|w| w.bits(0));
    }
    //delay(15_000_000); 
    d.delay_ms(10u32);


//#####################################################
    //TIMER0
    
    unsafe {
    // pac.TIMER0.tasks_stop.write(|w| w.tasks_stop().set_bit());
    pac.TIMER0.tasks_stop.write(|w| w.tasks_stop().set_bit());

    }
    //delay(15_000_000);  
    d.delay_ms(10u32);
    
    //#####################################################
    //TIMER1
    
    unsafe {
        pac.TIMER1.tasks_stop.write(|w| w.tasks_stop().set_bit());
    }
    //delay(10_000_000);  
    d.delay_ms(10u32);

   //#####################################################
    //RTC0
    
    unsafe {
        pac.RTC0.tasks_stop.write(|w| w.tasks_stop().set_bit());
    }
    //delay(15_000_000); 
    d.delay_ms(10u32);

   
  //#####################################################
    //RNG

    unsafe {
        pac.RNG.tasks_stop.write(|w| w.tasks_stop().set_bit());
    }
    //delay(15_000_000); 
    d.delay_ms(10u32);
 

   //#####################################################
    //USBD
    
    unsafe {
        pac.USBD.enable.write(|w| w.bits(0));
    }
    //delay(16_000_000); 
    d.delay_ms(10u32);

   //#####################################################
    unsafe {
        pac.QSPI.enable.write(|w| w.bits(0));
    }
    //delay(15_000_000); 
    d.delay_ms(10u32);

    //####################################################
    //GPIOTE 

    unsafe {
        pac.GPIOTE.intenclr.write(|w| w.port().set_bit());
        delay(10_000_000);
        d.delay_ms(10u32);
    }

    //####################################################
    //SPIM0   

        unsafe {
        pac.SPIM0.enable.write(|w| w.bits(0));
        }
        delay(10_000_000);
        d.delay_ms(10u32);

    //####################################################
 
    // disconnect GPIOs
    // POWER.SYSTEMOFF <= 1
 
        unsafe {
        pac.POWER.systemoff.write(|w| w.bits(1));     
        //delay(5_000_000);//ERWEITERT
        d.delay_ms(10u32);
        }
        //delay(10_000_000);//ERWEITERT
        d.delay_ms(10u32);
     
  
        loop {}

}