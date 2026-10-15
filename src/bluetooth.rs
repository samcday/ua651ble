use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use bluer::{
    Adapter, Address, Device, DeviceEvent, DeviceProperty, DiscoveryFilter, DiscoveryTransport,
    ErrorKind, UuidExt, agent::Agent, gatt::remote::Characteristic,
};
use chrono::{Datelike, Local, Timelike};
use futures_util::StreamExt;
use tokio::time::{sleep, timeout};

use crate::reading::{self, Reading};

const NAME_PREFIX: &str = "A&D_UA-651BLE_";

pub struct Bluetooth {
    session: bluer::Session,
    adapter: Adapter,
    device: Option<Device>,
}

impl Bluetooth {
    pub async fn new(adapter: &str) -> Result<Self> {
        let session = bluer::Session::new().await?;
        let adapter = session.adapter(adapter)?;
        ensure!(
            adapter.is_powered().await?,
            "The Bluetooth adapter is off. Turn it on before running this command"
        );
        adapter
            .set_discovery_filter(DiscoveryFilter {
                transport: DiscoveryTransport::Le,
                pattern: Some(NAME_PREFIX.into()),
                ..Default::default()
            })
            .await?;
        Ok(Self {
            session,
            adapter,
            device: None,
        })
    }

    async fn find(&self, address: Option<Address>, paired: bool) -> Result<Option<Device>> {
        let mut found = None;
        for candidate in self.adapter.device_addresses().await? {
            if address.is_some_and(|address| address != candidate) {
                continue;
            }
            let device = self.adapter.device(candidate)?;
            let name = device.name().await?.unwrap_or_default();
            if name.starts_with(NAME_PREFIX) && (!paired || device.is_paired().await?) {
                ensure!(
                    found.is_none(),
                    "More than one UA-651BLE found. Select one with --address"
                );
                found = Some(device);
            }
        }
        Ok(found)
    }

    async fn select(&mut self, device: Device) -> Result<Device> {
        ensure!(
            !device.is_blocked().await?,
            "The cuff is blocked in BlueZ. Unblock it before continuing"
        );
        ensure!(
            !device.is_connected().await?,
            "The cuff is already connected. Stop the other receiver first"
        );
        self.device = Some(device.clone());
        Ok(device)
    }

    pub async fn pair(&mut self, address: Option<Address>) -> Result<()> {
        if let Some(device) = self.find(address, true).await? {
            println!(
                "Already paired: {}. Run `ua651ble measure` before taking a reading.",
                device.address()
            );
            return Ok(());
        }
        eprintln!("Scanning. Hold START on the cuff until Pr appears.");
        let mut discovery = self.adapter.discover_devices_with_changes().await?;
        while discovery.next().await.is_some() {
            if let Some(device) = self.find(address, false).await? {
                let device = self.select(device).await?;
                let _agent = self.session.register_agent(Agent::default()).await?;
                device
                    .pair()
                    .await
                    .context("Pairing failed. Put the cuff in Pr mode and try again")?;
                write_clock(&device)
                    .await
                    .context("Pairing succeeded, but setting the cuff clock failed")?;
                println!(
                    "Paired {}. Run `ua651ble measure` before taking a reading.",
                    device.address()
                );
                return Ok(());
            }
        }
        bail!("Bluetooth discovery stopped before the cuff was found")
    }

    pub async fn measure(
        &mut self,
        address: Option<Address>,
        mut output: impl FnMut(&Reading) -> Result<()>,
    ) -> Result<()> {
        let device = self
            .find(address, true)
            .await?
            .context("No paired UA-651BLE found. Run `ua651ble pair` before `ua651ble measure`")?;
        let device = self.select(device).await?;
        let _discovery = self.adapter.discover_devices().await?;
        eprintln!("Waiting for {}. Take a measurement now.", device.address());
        connect(&device).await?;
        let mut events = device.events().await?;
        let characteristic = characteristic(&device, 0x2a35).await?;
        let mut readings = Box::pin(characteristic.notify().await?);
        let address = device.address().to_string();
        let mut received = false;
        loop {
            tokio::select! {
                // Drain queued readings before handling the disconnect.
                biased;
                bytes = readings.next() => {
                    let bytes = bytes.context("Blood pressure indication stream closed")?;
                    output(&reading::decode(&address, &bytes)?)?;
                    if !received {
                        match timeout(Duration::from_secs(2), write_clock(&device)).await {
                            Ok(Ok(())) => {}
                            result => eprintln!("Warning: clock synchronization failed: {result:?}"),
                        }
                    }
                    received = true;
                }
                event = events.next() => {
                    if matches!(event, None | Some(DeviceEvent::PropertyChanged(DeviceProperty::Connected(false)))) {
                        self.device = None;
                        break;
                    }
                }
            }
        }
        ensure!(received, "No reading received before the cuff disconnected");
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(device) = self.device.take() {
            // Also cancels a pending connection attempt.
            // BlueZ waits two seconds before disconnecting; allow time for completion.
            timeout(Duration::from_secs(10), device.disconnect())
                .await
                .context("Timed out while disconnecting the cuff")??;
        }
        Ok(())
    }
}

async fn connect(device: &Device) -> Result<()> {
    while let Err(error) = device.connect().await {
        match error.kind {
            ErrorKind::AlreadyConnected => break,
            ErrorKind::Failed | ErrorKind::InProgress | ErrorKind::ConnectionAttemptFailed => {
                sleep(Duration::from_secs(1)).await;
            }
            _ => return Err(error.into()),
        }
    }
    Ok(())
}

async fn characteristic(device: &Device, uuid: u16) -> Result<Characteristic> {
    for service in device.services().await? {
        for characteristic in service.characteristics().await? {
            if characteristic.uuid().await?.as_u16() == Some(uuid) {
                return Ok(characteristic);
            }
        }
    }
    bail!("The cuff does not expose characteristic {uuid:04X}")
}

async fn write_clock(device: &Device) -> Result<()> {
    let characteristic = characteristic(device, 0x2a08).await?;
    let now = Local::now();
    let mut data = (now.year() as u16).to_le_bytes().to_vec();
    data.extend([
        now.month() as u8,
        now.day() as u8,
        now.hour() as u8,
        now.minute() as u8,
        now.second() as u8,
    ]);
    characteristic.write(&data).await?;
    Ok(())
}
