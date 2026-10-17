//! Which Times Gate gets the panel, and what its screen showed before.

use std::path::Path;

use divoomkit::{ClockFace, Device, Discover, discover};
use serde::{Deserialize, Serialize};

use crate::config::DeviceConfig;

/// The device the panel goes to, and the identifier its clock face is kept
/// under — known even when the device itself came from memory.
#[derive(Debug)]
pub struct Located {
    pub device: Device,
    pub id: u32,
    pub remembered: bool,
}

/// Finds the device the config names. The sweep is paid once per start; it is
/// what tells the cloud's stale records from devices really on the network, and
/// it brings the id the clock face is looked up by.
pub fn locate(config: &DeviceConfig, memory: &Path) -> Result<Located, String> {
    let located = choose(discover(Discover::new()), config, recall_device(memory))?;
    if !located.remembered {
        remember_device(memory, &located.device)?;
    }
    Ok(located)
}

/// A sweep that saw nothing does not mean the device is gone: a process
/// started at login may find the network not up yet, or barred by the system
/// from the local network, while the device sits where it was last seen. The
/// address it was last found at is tried then — but never in place of an
/// answer the sweep did give.
pub fn choose(
    found: divoomkit::Result<Vec<Device>>,
    config: &DeviceConfig,
    memory: Option<Known>,
) -> Result<Located, String> {
    let fresh = |device: Device| Located {
        id: device.id(),
        device,
        remembered: false,
    };
    let err = match found {
        Ok(found) => return pick(&found, config).map(fresh),
        Err(err) => err,
    };

    let from_memory = |known: &Known| -> Option<Located> {
        Some(Located {
            device: Device::at(known.ip.parse().ok()?),
            id: known.id,
            remembered: true,
        })
    };
    match (&config.ip, &config.name, memory) {
        (Some(ip), _, Some(known)) if known.ip == *ip => {
            from_memory(&known).ok_or_else(|| describe(&err))
        }
        (Some(ip), _, _) => Ok(fresh(Device::at(
            ip.parse().map_err(|_| format!("bad device.ip {ip}"))?,
        ))),
        (None, Some(name), Some(known)) if known.name.eq_ignore_ascii_case(name) => {
            from_memory(&known).ok_or_else(|| describe(&err))
        }
        (None, None, Some(known)) => from_memory(&known).ok_or_else(|| describe(&err)),
        _ => Err(describe(&err)),
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

pub fn key(located: &Located, screen: u8) -> String {
    match located.id {
        0 => format!("ip:{}/screen:{screen}", located.device.ip()),
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

/// Where the device was last found. Kept beside the config, not in it: the
/// config is the same file on every machine, while the address belongs to one
/// network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Known {
    pub ip: String,
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub name: String,
}

pub fn recall_device(path: &Path) -> Option<Known> {
    let data = std::fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

/// A device seen without its identifier at the address memory holds keeps the
/// identifier memory has for it: the clock face is filed under it.
pub fn remember_device(path: &Path, device: &Device) -> Result<(), String> {
    let ip = device.ip().to_string();
    let previous = recall_device(path).filter(|known| known.ip == ip);
    let known = Known {
        id: match device.id() {
            0 => previous.as_ref().map_or(0, |known| known.id),
            id => id,
        },
        name: match device.name() {
            "" => previous.map(|known| known.name).unwrap_or_default(),
            name => name.to_string(),
        },
        ip,
    };
    let data = serde_json::to_vec_pretty(&known).map_err(|err| err.to_string())?;
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
        let device = Located {
            device: at("10.0.0.5"),
            id: 0,
            remembered: false,
        };
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

    fn known(ip: &str, id: u32, name: &str) -> Known {
        Known {
            ip: ip.into(),
            id,
            name: name.into(),
        }
    }

    #[test]
    fn files_the_clock_face_under_the_identifier_when_there_is_one() {
        let located = Located {
            device: at("10.0.0.5"),
            id: 42,
            remembered: true,
        };
        assert_eq!(key(&located, 1), "id:42/screen:1");
    }

    #[test]
    fn prefers_what_the_sweep_found_to_what_memory_holds() {
        let memory = Some(known("10.0.0.9", 7, "Times Gate"));
        let located = choose(Ok(vec![at("10.0.0.5")]), &config(None, None), memory).unwrap();
        assert_eq!(located.device.ip().to_string(), "10.0.0.5");
        assert!(!located.remembered);
    }

    #[test]
    fn falls_back_to_the_last_address_when_the_sweep_sees_nothing() {
        let memory = || Some(known("10.0.0.9", 7, "Times Gate"));
        let nothing = || Err(divoomkit::Error::NotFound);

        let located = choose(nothing(), &config(None, None), memory()).unwrap();
        assert_eq!(located.device.ip().to_string(), "10.0.0.9");
        assert_eq!(located.id, 7);
        assert!(located.remembered);

        let named = choose(nothing(), &config(None, Some("times gate")), memory()).unwrap();
        assert_eq!(named.id, 7);

        let err = choose(nothing(), &config(None, Some("Kitchen")), memory()).unwrap_err();
        assert!(err.contains("no Divoom devices"), "{err}");

        let err = choose(nothing(), &config(None, None), None).unwrap_err();
        assert!(err.contains("no Divoom devices"), "{err}");
    }

    #[test]
    fn keeps_the_identifier_of_a_configured_address_it_has_seen() {
        let nothing = || Err(divoomkit::Error::NotFound);
        let memory = || Some(known("10.0.0.9", 7, ""));

        let same = choose(nothing(), &config(Some("10.0.0.9"), None), memory()).unwrap();
        assert_eq!((same.id, same.remembered), (7, true));

        let other = choose(nothing(), &config(Some("10.0.0.8"), None), memory()).unwrap();
        assert_eq!(other.device.ip().to_string(), "10.0.0.8");
        assert_eq!(other.id, 0);
    }

    #[test]
    fn remembers_where_the_device_was_found() {
        let path =
            std::env::temp_dir().join(format!("sentrypanel-known-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(recall_device(&path), None);

        remember_device(&path, &at("10.0.0.5")).unwrap();
        assert_eq!(recall_device(&path), Some(known("10.0.0.5", 0, "")));

        std::fs::write(&path, r#"{"ip":"10.0.0.5","id":7,"name":"Times Gate"}"#).unwrap();
        remember_device(&path, &at("10.0.0.5")).unwrap();
        assert_eq!(
            recall_device(&path),
            Some(known("10.0.0.5", 7, "Times Gate")),
            "what is known of the same address survives a plain sighting"
        );

        remember_device(&path, &at("10.0.0.6")).unwrap();
        assert_eq!(recall_device(&path), Some(known("10.0.0.6", 0, "")));

        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(recall_device(&path), None);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn words_the_errors_of_the_device() {
        assert!(describe(&divoomkit::Error::NotFound).contains("no Divoom devices"));
        assert!(describe(&divoomkit::Error::NoSuchScreen(7)).contains('7'));
    }
}
