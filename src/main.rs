use serde::Deserialize;
use std::thread::sleep;
use std::time::Duration;
use time::{format_description::well_known::Iso8601, OffsetDateTime};

use embedded_graphics::{
    mono_font::{ascii::{FONT_6X10, FONT_6X13_BOLD}, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::Rectangle,
    text::{Baseline, Text},
};
use sh1106::{prelude::*, Builder};

use anyhow::{self};
use embedded_svc::http::client::Client;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::i2c::*;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use esp_idf_svc::io::Write;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sntp::{self, SyncStatus};
use esp_idf_svc::wifi::{BlockingWifi, ClientConfiguration, Configuration, EspWifi};

#[toml_cfg::toml_config]
pub struct Config {
    #[default("Wokwi-GUEST")]
    wifi_ssid: &'static str,
    #[default("")]
    wifi_psk: &'static str,
    #[default("")]
    from_place1: &'static str,
    #[default("")]
    to_place1: &'static str,
    #[default("")]
    from_place2: &'static str,
    #[default("")]
    to_place2: &'static str,
}
const NORMAL_TEXT_STYLE: MonoTextStyle<'_, BinaryColor> = MonoTextStyleBuilder::new()
    .font(&FONT_6X10)
    .text_color(BinaryColor::On)
    .background_color(BinaryColor::Off)
    .build();
const INVERTED_TEXT_STYLE: MonoTextStyle<'_, BinaryColor> = MonoTextStyleBuilder::new()
    .font(&FONT_6X10)
    .text_color(BinaryColor::Off)
    .background_color(BinaryColor::On)
    .build();
const INVERTED_TITLE_TEXT_STYLE: MonoTextStyle<'_, BinaryColor> = MonoTextStyleBuilder::new()
    .font(&FONT_6X13_BOLD)
    .text_color(BinaryColor::Off)
    .background_color(BinaryColor::On)
    .build();

fn display(
    departures: Vec<Departure>,
    display_interface: &mut GraphicsMode<I2cInterface<I2cDriver<'_>>>,
) -> anyhow::Result<()> {
    match display_interface.flush() {
        Ok(_) => (),
        Err(err) => log::error!("display: flush 1: {:?}", err),
    };
    display_interface.fill_solid(
        &Rectangle::new(Point::new(1, 12), Size::new(64, 112)),
        BinaryColor::Off,
    )?;

    for (i, d) in departures.iter().enumerate() {
		{
			display_interface.fill_solid(
				&Rectangle::new(Point::new(6, (i as i32 * 12) + 16), Size::new(1, 10)),
				BinaryColor::On,
			)?;
			match Text::with_baseline(
				format!("{}", d.line_number).as_str(),
				Point::new(7, (i as i32 * 12) + 16),
				INVERTED_TEXT_STYLE,
				Baseline::Top,
			)
				.draw(display_interface)
				{
					Ok(_) => (),
					Err(err) => log::error!("display: draw: {:?}", err),
				};
		}
        match Text::with_baseline(
            format!("{: >5}", d.leaving_in).as_str(),
            Point::new(32, (i as i32 * 12) + 16),
            NORMAL_TEXT_STYLE,
            Baseline::Top,
        )
        .draw(display_interface)
        {
            Ok(_) => (),
            Err(err) => log::error!("display: draw: {:?}", err),
        };
    }

    match display_interface.flush() {
        Ok(_) => (),
        Err(err) => log::error!("display: flush 2: {:?}", err),
    };
    Ok(())
}

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();
    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    // Set up WiFi
    let peripherals = Peripherals::take()?;
    let _wifi = match _esp_wifi_setup(peripherals.modem) {
        Ok(w) => {
            log::info!("WiFi Successfully Connected!");
            w
        }
        Err(err) => {
            log::error!("Could not connect to WiFi!");
            return Err(err);
        }
    };

    let sntp = sntp::EspSntp::new_default()?;
    log::info!("SNTP initialized, waiting for status!");
    while sntp.get_sync_status() != SyncStatus::Completed {}
    log::info!("SNTP status received!");

    let i2c = peripherals.i2c0;
    let sda = peripherals.pins.gpio21;
    let scl = peripherals.pins.gpio22;
    let config = I2cConfig::new().baudrate(100.kHz().into());
    let i2c = I2cDriver::new(i2c, sda, scl, &config)?;
    log::info!("I2C driver configured!");

    log::info!("Initiliazing SH1106 display...");
    let mut display_interface: GraphicsMode<_> = Builder::new().connect_i2c(i2c).into();
    match display_interface.set_rotation(DisplayRotation::Rotate90) {
        Ok(_) => (),
        Err(err) => log::error!("display: set_rotation: {:?}", err),
    };
    match display_interface.init() {
        Ok(_) => (),
        Err(err) => log::error!("display: init: {:?}", err),
    };
    display_interface.fill_solid(
        &Rectangle::new(Point::new(0, 0), Size::new(64, 12)),
        BinaryColor::On,
    )?;
    match Text::with_baseline(
        "LEAVING IN ",
        Point::new(2, 0 as i32 * 12),
        INVERTED_TITLE_TEXT_STYLE,
        Baseline::Top,
    )
    .draw(&mut display_interface)
    {
        Ok(_) => (),
        Err(err) => log::error!("display: draw: {:?}", err),
    };

    log::info!("SH1106 display initialized!");

    log::info!("Initialization complete!");

    // Start application
    const SLEEP_SECONDS: u64 = 20;
    loop {
        let data = match client() {
            Ok(val) => val,
            Err(err) => {
                log::error!("{}", err);
                log::info!("Sleeping for {} Seconds", SLEEP_SECONDS);
                sleep(Duration::from_secs(SLEEP_SECONDS));
                continue;
            }
        };
        for _ in 0..(SLEEP_SECONDS * 2) {
            let departures = Departure::from_top_level_data(data.clone());
            log::info!("Response json: {:?}", departures);
            match display(departures, &mut display_interface) {
                Ok(x) => x,
                Err(err) => log::error!("{}", err),
            };
            sleep(Duration::from_millis(500))
        }
    }
}

fn client() -> anyhow::Result<TopLevelData> {
    fn make_query(from: &str, to: &str) -> String {
        format!(
            r#"
			trip(
				from: {{
					place: "{}"
				}},
				to: {{
					place: "{}"
				}},
				numTripPatterns: 4
				modes: {{
					accessMode: foot
					egressMode: foot
					transportModes: [{{
						transportMode: bus
						transportSubModes: [localBus]
					}}]
				}}
			) {{
				tripPatterns {{
					legs {{
						expectedStartTime
						line {{
							publicCode
						}}
					}}
				}}
			}}
		"#,
            from, to
        )
    }
    let cfg = HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        ..Default::default()
    };
    let conn = EspHttpConnection::new(&cfg)?;
    let mut client = Client::wrap(conn);

    let url = "https://api.entur.io/journey-planner/v3/graphql";
    let headers = [
        ("content-type", "application/json"),
        ("ET-Client-Name", "eilefsen-entur_display"),
    ];
    let query = format!(
        r#"{{
			trip1: {}, trip2: {}
		}}"#,
        make_query(CONFIG.from_place1, CONFIG.to_place1),
        make_query(CONFIG.from_place2, CONFIG.to_place2),
    );
    log::info!("query: {}", query);
    let json = serde_json::json!({"query": query});
    let mut request = client.post(url, &headers)?;
    request.write_fmt(format_args!("{}", json))?;
    let mut response = request.submit()?;
    let mut buffer = [0; 2048];
    response.read(&mut buffer)?;
    let c = String::from_utf8_lossy(&buffer);
    let content = c.trim_matches('\0');
    log::info!("Response content: {:?}", content);
    let status = response.status();
    log::info!("Response status code: {}", status);

    let response_data: TopLevelData = serde_json::from_str(content)?;

    Ok(response_data)
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct TopLevelData {
    data: Data,
}
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct Data {
    trip1: Trip,
    trip2: Trip,
}
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct Trip {
    trip_patterns: Vec<TripPattern>,
}
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct TripPattern {
    legs: Vec<Leg>,
}
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct Leg {
    expected_start_time: String,
    line: Line,
}
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct Line {
    public_code: String,
}

#[derive(Debug)]
struct Departure {
    start_time: OffsetDateTime,
    leaving_in: String,
    line_number: String,
}
impl Departure {
    fn from_top_level_data(data: TopLevelData) -> Vec<Departure> {
        let mut departures = Departure::from_trip(data.data.trip1);
        departures.append(&mut Departure::from_trip(data.data.trip2));
        departures
    }
    fn from_trip(trip: Trip) -> Vec<Departure> {
        trip.trip_patterns
            .into_iter()
            .flat_map(|tp| tp.legs)
            .filter_map(|leg| Departure::from_leg(leg).ok())
            .collect()
    }
    fn from_leg(leg: Leg) -> anyhow::Result<Departure> {
        let start = OffsetDateTime::parse(leg.expected_start_time.as_str(), &Iso8601::DEFAULT)?;
        let now = OffsetDateTime::now_utc();

        log::info!("{}", now);
        let diff = start - now;
        let leaving = format!(
            "{}:{:02}",
            diff.whole_minutes(),
            diff.whole_seconds() - (diff.whole_minutes() * 60)
        );
        Ok(Departure {
            start_time: start,
            leaving_in: leaving,
            line_number: leg.line.public_code,
        })
    }
}

fn _esp_wifi_setup(modem: Modem) -> anyhow::Result<BlockingWifi<EspWifi<'static>>> {
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = BlockingWifi::wrap(EspWifi::new(modem, sysloop.clone(), Some(nvs))?, sysloop)?;

    let cfg = Configuration::Client(ClientConfiguration {
        ssid: heapless::String::try_from(CONFIG.wifi_ssid).unwrap(),
        password: heapless::String::try_from(CONFIG.wifi_psk).unwrap(),
        auth_method: esp_idf_svc::wifi::AuthMethod::None,
        ..Default::default()
    });

    wifi.set_configuration(&cfg)?;
    wifi.start()?;
    wifi.connect()?;
    wifi.wait_netif_up()?;
    // Print Out Wifi Connection Configuration
    while !wifi.is_connected().unwrap() {
        let config = wifi.get_configuration().unwrap();
        println!("Waiting for station {:?}", config);
    }

    Ok(wifi)
}
