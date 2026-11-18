// Outil de diagnostic : liste les périphériques d'entrée et leur configuration
// par défaut. `cargo run --example devices`
use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    let host = cpal::default_host();
    println!("hôte : {:?}", host.id());
    match host.default_input_device() {
        Some(d) => println!("entrée par défaut : {:?}", d.id()),
        None => println!("entrée par défaut : AUCUNE"),
    }
    match host.input_devices() {
        Ok(devices) => {
            for (i, d) in devices.enumerate() {
                let id = d.id().map(|v| v.to_string()).unwrap_or_default();
                match d.default_input_config() {
                    Ok(c) => println!(
                        "[{i}] {id} — {} canaux, {} Hz, format {:?}",
                        c.channels(),
                        c.sample_rate(),
                        c.sample_format()
                    ),
                    Err(e) => println!("[{i}] {id} — pas de configuration d'entrée ({e})"),
                }
            }
        }
        Err(e) => println!("impossible d'énumérer les entrées : {e}"),
    }
}
