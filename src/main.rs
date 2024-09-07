use std::path::Path;

use anyhow::{bail, Context, Error};
use argh::FromArgs;
use async_udev::AsyncMonitorSocket;
use evdev::{InputEventKind, Key};
use tokio::{process::Command, select};
use tokio_stream::{StreamExt, StreamMap};
use udev::{Enumerator, MonitorBuilder};

mod async_udev;

const UDEV_TAG: &str = "button-ears";
const TARGET_KEYS: &[Key] = &[Key::KEY_SLEEP, Key::KEY_SUSPEND, Key::KEY_POWER, Key::KEY_B];

#[derive(FromArgs)]
/// Listen for power button events
struct ButtonEars {
    #[argh(positional)]
    /// program to execute
    command: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: ButtonEars = argh::from_env();

    let mut enumerator = Enumerator::new()?;
    enumerator.match_tag(UDEV_TAG)?;

    let mut stream_map = StreamMap::new();
    for udev in enumerator
        .scan_devices()
        .context("Could not enumerate udev devices")?
    {
        let devpath = Path::new(udev.devpath()).to_path_buf();
        let Some(devnode) = udev.devnode() else {
            eprintln!("Device has no devnode: {}", devpath.display());
            continue;
        };
        let evdev = evdev::Device::open(devnode)
            .with_context(|| format!("Could not open device {}", devnode.display()))?;
        if let Some(supported_keys) = evdev.supported_keys() {
            if !TARGET_KEYS.iter().any(|key| supported_keys.contains(*key)) {
                bail!(
                    "Device {} does not support any of TARGET_KEYS",
                    devnode.display()
                )
            }
        }
        let stream = evdev
            .into_event_stream()
            .with_context(|| format!("Could not make stream for dev {}", devpath.display()))?;
        stream_map.insert(devpath, stream);
    }

    let mut monitor: AsyncMonitorSocket = MonitorBuilder::new()?
        .match_tag(UDEV_TAG)?
        .listen()?
        .try_into()?;

    loop {
        eprintln!("loop");
        select! {
            Some(ev_udev) = monitor.next() => {
                eprintln!("Got udev event: {ev_udev:?}")
                // TODO add to stream_map
            },
            Some((key, ev_input)) = stream_map.next(), if !stream_map.is_empty() => {
                eprintln!("Got input event: {ev_input:?}");
                match ev_input {
                    Ok(ev) => {
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
                    },
                    Err(_) => {
                        stream_map.remove(&key);
                    }
                }
            }
        }
    }
}
