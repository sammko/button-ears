use anyhow::{bail, Context, Error};
use argh::FromArgs;
use evdev::{InputEventKind, Key};
use futures::{stream::SelectAll, StreamExt};
use tokio::process::Command;

const TARGET_KEYS: &[Key] = &[Key::KEY_SLEEP, Key::KEY_SUSPEND, Key::KEY_POWER];

#[derive(FromArgs)]
/// Listen for power button events
struct ButtonEars {
    #[argh(option, short = 'c')]
    /// program to execute
    command: String,

    #[argh(positional)]
    /// input devices
    devices: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: ButtonEars = argh::from_env();

    if args.devices.len() == 0 {
        bail!("No devices specified")
    }

    let mut stream = SelectAll::new();
    for path in &args.devices {
        let device =
            evdev::Device::open(path).with_context(|| format!("Could not open device {path}"))?;
        if let Some(supported_keys) = device.supported_keys() {
            if !TARGET_KEYS.iter().any(|key| supported_keys.contains(*key)) {
                bail!("Device {path} does not support any of TARGET_KEYS")
            }
        }
        stream.push(
            device
                .into_event_stream()
                .with_context(|| format!("Could not make stream for dev {path}"))?,
        )
    }

    while let Some(ev) = stream.next().await {
        let ev = ev.context("Failed to read event")?;
        let ev_kind = ev.kind();
        if TARGET_KEYS
            .iter()
            .any(|key| ev_kind == InputEventKind::Key(*key))
            && ev.value() == 0
        {
            match Command::new(&args.command).status().await {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Command failed: {e}")
                }
            }
        }
    }

    unreachable!()
}
