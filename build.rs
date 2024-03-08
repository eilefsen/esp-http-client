#[toml_cfg::toml_config]
pub struct Config {
    #[default("")]
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

fn main() {
    // Check if the `cfg.toml` file exists and has been filled out.
    if !std::path::Path::new("cfg.toml").exists() {
        panic!("You need to create a `cfg.toml` file with your Wi-Fi credentials! Use `cfg.toml.example` as a template.");
    }

    embuild::espidf::sysenv::output();
}
