//! Which Times Gate gets the panel, and what its screen showed before.

use std::path::Path;

use divoomkit::{ClockFace, Device, Discover, discover};
use serde::{Deserialize, Serialize};

use crate::config::DeviceConfig;

/// Finds the device the config names. The sweep is paid once per start; it is
/// what tells the cloud's stale records from devices really on the network, and
/// it brings the id the clock face is looked up by.
pub fn locate(config: &DeviceConfig) -> Result<Device, String> {
    match discover(Discover::new()) {
        Ok(found) => pick(&found, config),
        Err(err) => match &config.ip {
            Some(ip) => Ok(Device::at(
                ip.parse().map_err(|_| format!("bad device.ip {ip}"))?,
            )),
            None => Err(describe(&err)),
        },
    }
}

pub fn pick(found: &[Device], config: &DeviceConfig) -> Result<Device, String> {
    if let Some(ip) = &config.ip {
        return Ok(found
            .iter()
            .find(|device| device.ip().to_string() == *ip)
            .cloned()
            .unwrap_or_else(|| Device::at(ip.parse().expect("validated with the config"))));
    }
    if let Some(name) = &config.name {
        return found
            .iter()
            .find(|device| device.name().eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| format!("no device named {name:?} on the network\n{}", list(found)));
    }
    match found {
        [only] => Ok(only.clone()),
        [] => Err("no Divoom devices are visible on this network".into()),
        _ => Err(format!(
            "more than one device — set device.ip or device.name in the config\n{}",
            list(found)
        )),
    }
}

pub fn list(found: &[Device]) -> String {
    found
        .iter()
        .enumerate()
        .map(|(index, device)| {
            let name = if device.name().is_empty() {
                "Divoom device"
            } else {
                device.name()
            };
            format!("  {}. {name} — {}\n", index + 1, device.ip())
        })
        .collect()
}

/// The crate speaks of the device in its own words; these are the ones the
/// user reads.
pub fn describe(err: &divoomkit::Error) -> String {
    match err {
        divoomkit::Error::NotFound => "no Divoom devices are visible on this network".into(),
        other => other.to_string(),
    }
}

/// What the screen showed before the panel took it. Once the panel is there,
/// the cloud reports the panel, so the answer has to be kept from the first
/// start for every start after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Remembered {
    key: String,
    clock_id: u32,
    independence: u32,
}

pub fn key(device: &Device, screen: u8) -> String {
    match device.id() {
        0 => format!("ip:{}/screen:{screen}", device.ip()),
        id => format!("id:{id}/screen:{screen}"),
    }
}

pub fn recall(path: &Path, key: &str) -> Option<ClockFace> {
    let data = std::fs::read(path).ok()?;
    let remembered: Remembered = serde_json::from_slice(&data).ok()?;
    (remembered.key == key).then_some(ClockFace {
        id: remembered.clock_id,
        independence: remembered.independence,
    })
}

pub fn remember(path: &Path, key: &str, face: ClockFace) -> Result<(), String> {
    let remembered = Remembered {
        key: key.to_string(),
        clock_id: face.id,
        independence: face.independence,
    };
    let data = serde_json::to_vec_pretty(&remembered).map_err(|err| err.to_string())?;
    std::fs::write(path, data).map_err(|err| format!("could not write {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn at(ip: &str) -> Device {
        Device::at(ip.parse::<IpAddr>().unwrap())
    }

    fn config(ip: Option<&str>, name: Option<&str>) -> DeviceConfig {
        DeviceConfig {
            ip: ip.map(String::from),
            name: name.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn takes_the_only_device() {
        let found = [at("10.0.0.5")];
        assert_eq!(
            pick(&found, &config(None, None)).unwrap().ip(),
            found[0].ip()
        );
    }

    #[test]
    fn asks_which_one_when_there_are_several() {
        let found = [at("10.0.0.5"), at("10.0.0.6")];
        let err = pick(&found, &config(None, None)).unwrap_err();
        assert!(err.contains("device.ip"), "{err}");
        assert!(err.contains("2. Divoom device — 10.0.0.6"), "{err}");

        assert!(pick(&[], &config(None, None)).is_err());
    }

    #[test]
    fn finds_the_device_by_its_address_even_when_the_sweep_missed_it() {
        let found = [at("10.0.0.5"), at("10.0.0.6")];
        let chosen = pick(&found, &config(Some("10.0.0.6"), None)).unwrap();
        assert_eq!(chosen.ip().to_string(), "10.0.0.6");

        let missed = pick(&found, &config(Some("10.0.0.9"), None)).unwrap();
        assert_eq!(missed.ip().to_string(), "10.0.0.9");
    }

    #[test]
    fn says_when_no_device_has_the_name() {
        let err = pick(&[at("10.0.0.5")], &config(None, Some("Kitchen"))).unwrap_err();
        assert!(err.contains("Kitchen"), "{err}");
    }

    #[test]
    fn remembers_the_clock_face_of_one_screen_of_one_device() {
        let path =
            std::env::temp_dir().join(format!("sentrypanel-face-{}.json", std::process::id()));
        let device = at("10.0.0.5");
        let (first, second) = (key(&device, 0), key(&device, 1));
        assert_eq!(first, "ip:10.0.0.5/screen:0");

        assert_eq!(recall(&path, &first), None);
        let face = ClockFace {
            id: 104,
            independence: 745289,
        };
        remember(&path, &first, face).unwrap();

        assert_eq!(recall(&path, &first), Some(face));
        assert_eq!(
            recall(&path, &second),
            None,
            "another screen showed something else"
        );

        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(recall(&path, &first), None);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn words_the_errors_of_the_device() {
        assert!(describe(&divoomkit::Error::NotFound).contains("no Divoom devices"));
        assert!(describe(&divoomkit::Error::NoSuchScreen(7)).contains('7'));
    }
}
