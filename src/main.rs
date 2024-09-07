use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Error};
use argh::FromArgs;
use async_udev::AsyncMonitorSocket;
use evdev::{EventStream, InputEventKind, Key};
use tokio::{process::Command, select};
use tokio_stream::{StreamExt, StreamMap};
use udev::{Enumerator, MonitorBuilder};

mod async_udev;

const UDEV_TAG: &str = "button-ears";
const TARGET_KEYS: &[Key] = &[Key::KEY_SLEEP, Key::KEY_SUSPEND, Key::KEY_POWER];

#[derive(FromArgs)]
/// Listen for power button events
struct ButtonEars {
    #[argh(positional)]
    /// program to execute
    command: String,
}

fn add_to_stream_map(
    stream_map: &mut StreamMap<PathBuf, EventStream>,
    udev: &udev::Device,
) -> Result<(), Error> {
    let devpath = Path::new(udev.devpath()).to_path_buf();
    let Some(devnode) = udev.devnode() else {
        bail!("Device has no devnode: {}", devpath.display());
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

    eprintln!("Adding device {}", devpath.display());
    stream_map.insert(devpath, stream);

    Ok(())
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
        if let Err(err) = add_to_stream_map(&mut stream_map, &udev).context("Failed to add device")
        {
            eprintln!("{:?}", err);
        }
    }

    let mut monitor: AsyncMonitorSocket = MonitorBuilder::new()?
        .match_tag(UDEV_TAG)?
        .listen()?
        .try_into()?;

    loop {
        select! {
            Some(ev) = monitor.next() => {
                let ev = ev.context("Udev monitor died")?;
                if ev.event_type() == udev::EventType::Add {
                    if let Err(err) = add_to_stream_map(&mut stream_map, &ev.device()).context("Failed to add device") {
                        eprintln!("{:?}", err)
                    }
                }
            },
            Some((key, ev)) = stream_map.next(), if !stream_map.is_empty() => {
                match ev {
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
                        eprintln!("Removing device {}", key.display());
                        stream_map.remove(&key);
                    }
                }
            }
        }
    }
}
