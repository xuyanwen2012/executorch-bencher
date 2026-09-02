//! The one place that turns a flat, all-optional description of a host into
//! a validated [`HostState`], enforcing the platform and device-class rules
//! the database CHECK also enforces - by field, so a violation is reported
//! as `details.field` rather than as a constraint failure. Shared by the
//! HTTP write path and the observer-log importer so neither can write a
//! row the other would reject. See `specs/benchmark-schema` - "Host state
//! is captured as a platform-specific immutable snapshot".

use crate::domain::{DeviceClass, Platform};
use crate::runs::{AndroidDeviceState, AndroidLabConfig, HostState, LinuxHostState};

/// A validation failure naming the request field it concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: &'static str,
    pub message: String,
}

pub fn field_err(field: &'static str, message: impl Into<String>) -> FieldError {
    FieldError {
        field,
        message: message.into(),
    }
}

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for FieldError {}

/// Every host field a caller may supply, before any platform rule is
/// applied. Field names match the wire and column names so errors can
/// name them directly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostInput {
    pub host_os: Option<String>,
    pub host_kernel: Option<String>,
    pub host_cpu_model: Option<String>,
    pub host_cpu_count: Option<i64>,
    pub host_memory_bytes: Option<i64>,
    pub host_accelerator: Option<String>,
    pub host_accelerator_driver: Option<String>,
    pub device_uptime_seconds: Option<i64>,
    pub thermal_throttling: Option<bool>,
    pub bsp_version: Option<String>,
    pub sumd_driver_version: Option<String>,
    pub gpu_clock_mhz: Option<i64>,
    pub mif_clock_mhz: Option<i64>,
    pub int_clock_mhz: Option<i64>,
    pub battery_charging: Option<bool>,
    pub initial_temperature_celsius: Option<f64>,
    pub max_temperature_celsius: Option<f64>,
}

pub fn non_negative(field: &'static str, value: i64) -> Result<i64, FieldError> {
    if value < 0 {
        Err(field_err(field, "must not be negative"))
    } else {
        Ok(value)
    }
}

fn positive_clock(field: &'static str, value: Option<i64>) -> Result<Option<i64>, FieldError> {
    match value {
        Some(v) if v <= 0 => Err(field_err(field, "must be greater than zero MHz")),
        other => Ok(other),
    }
}

fn temperature(field: &'static str, value: Option<f64>) -> Result<Option<f64>, FieldError> {
    match value {
        Some(v) if !(-40.0..=150.0).contains(&v) => {
            Err(field_err(field, "must be between -40 and 150 degrees Celsius"))
        }
        other => Ok(other),
    }
}

fn required<T>(field: &'static str, value: Option<T>, why: &str) -> Result<T, FieldError> {
    value.ok_or_else(|| field_err(field, format!("required {why}")))
}

fn forbidden<T>(field: &'static str, value: &Option<T>, why: &str) -> Result<(), FieldError> {
    if value.is_some() {
        Err(field_err(field, format!("must be null {why}")))
    } else {
        Ok(())
    }
}

/// Builds the platform-specific snapshot from `input`, reporting the first
/// rule violated by field.
pub fn host_state(
    input: &HostInput,
    platform: Platform,
    device_class: DeviceClass,
) -> Result<HostState, FieldError> {
    let gpu = positive_clock("gpu_clock_mhz", input.gpu_clock_mhz)?;
    let mif = positive_clock("mif_clock_mhz", input.mif_clock_mhz)?;
    let int = positive_clock("int_clock_mhz", input.int_clock_mhz)?;
    let initial = temperature("initial_temperature_celsius", input.initial_temperature_celsius)?;
    let max = temperature("max_temperature_celsius", input.max_temperature_celsius)?;
    if let Some(uptime) = input.device_uptime_seconds {
        non_negative("device_uptime_seconds", uptime)?;
    }
    if let Some(n) = input.host_cpu_count
        && n <= 0
    {
        return Err(field_err("host_cpu_count", "must be greater than zero"));
    }
    if let Some(n) = input.host_memory_bytes {
        non_negative("host_memory_bytes", n)?;
    }

    match platform {
        Platform::Linux => {
            let why = "on a linux run";
            forbidden("bsp_version", &input.bsp_version, why)?;
            forbidden("sumd_driver_version", &input.sumd_driver_version, why)?;
            forbidden("gpu_clock_mhz", &gpu, why)?;
            forbidden("mif_clock_mhz", &mif, why)?;
            forbidden("int_clock_mhz", &int, why)?;
            forbidden("battery_charging", &input.battery_charging, why)?;
            forbidden("initial_temperature_celsius", &initial, why)?;
            forbidden("max_temperature_celsius", &max, why)?;
            Ok(HostState::Linux(LinuxHostState {
                os: required("host_os", input.host_os.clone(), why)?,
                kernel: required("host_kernel", input.host_kernel.clone(), why)?,
                cpu_model: required("host_cpu_model", input.host_cpu_model.clone(), why)?,
                cpu_count: input.host_cpu_count,
                memory_bytes: input.host_memory_bytes,
                accelerator: required("host_accelerator", input.host_accelerator.clone(), why)?,
                accelerator_driver: input.host_accelerator_driver.clone(),
                uptime_seconds: input.device_uptime_seconds,
                thermal_throttling: input.thermal_throttling,
            }))
        }
        Platform::Android => {
            let lab_fields = [
                ("bsp_version", input.bsp_version.is_some()),
                ("sumd_driver_version", input.sumd_driver_version.is_some()),
                ("gpu_clock_mhz", gpu.is_some()),
                ("mif_clock_mhz", mif.is_some()),
                ("int_clock_mhz", int.is_some()),
            ];
            let first_missing = || {
                lab_fields
                    .iter()
                    .find(|(_, present)| !*present)
                    .map(|(field, _)| *field)
                    .unwrap_or("bsp_version")
            };
            let present = lab_fields.iter().filter(|(_, p)| *p).count();
            let lab = if present == lab_fields.len() {
                Some(AndroidLabConfig {
                    bsp_version: input.bsp_version.clone().unwrap_or_default(),
                    sumd_driver_version: input.sumd_driver_version.clone().unwrap_or_default(),
                    gpu_clock_mhz: gpu.unwrap_or_default(),
                    mif_clock_mhz: mif.unwrap_or_default(),
                    int_clock_mhz: int.unwrap_or_default(),
                })
            } else if present == 0 {
                None
            } else {
                return Err(field_err(
                    first_missing(),
                    "BSP version, SUMD driver version, and the GPU/MIF/INT clocks are recorded all together or not at all",
                ));
            };
            let state = AndroidDeviceState {
                os: input.host_os.clone(),
                kernel: input.host_kernel.clone(),
                soc: input.host_cpu_model.clone(),
                cpu_count: input.host_cpu_count,
                memory_bytes: input.host_memory_bytes,
                gpu: input.host_accelerator.clone(),
                gpu_driver: input.host_accelerator_driver.clone(),
                uptime_seconds: input.device_uptime_seconds,
                battery_charging: input.battery_charging,
                initial_temperature_celsius: initial,
                max_temperature_celsius: max,
                thermal_throttling: input.thermal_throttling,
                lab,
            };
            if device_class == DeviceClass::Internal {
                let why = "on an internal android device";
                if state.lab.is_none() {
                    return Err(field_err(first_missing(), format!("required {why}")));
                }
                required("device_uptime_seconds", state.uptime_seconds, why)?;
                required("battery_charging", state.battery_charging, why)?;
                required("initial_temperature_celsius", state.initial_temperature_celsius, why)?;
                required("max_temperature_celsius", state.max_temperature_celsius, why)?;
                required("thermal_throttling", state.thermal_throttling, why)?;
            }
            Ok(HostState::Android(state))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux_input() -> HostInput {
        HostInput {
            host_os: Some("Ubuntu 24.04".into()),
            host_kernel: Some("6.8.0".into()),
            host_cpu_model: Some("Ryzen 7".into()),
            host_accelerator: Some("RTX 4070".into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_linux_host_needs_os_kernel_cpu_and_accelerator() {
        assert!(host_state(&linux_input(), Platform::Linux, DeviceClass::External).is_ok());
        let mut missing = linux_input();
        missing.host_accelerator = None;
        let err = host_state(&missing, Platform::Linux, DeviceClass::External).unwrap_err();
        assert_eq!(err.field, "host_accelerator");
    }

    #[test]
    fn a_linux_host_rejects_android_lab_fields() {
        let mut input = linux_input();
        input.gpu_clock_mhz = Some(980);
        let err = host_state(&input, Platform::Linux, DeviceClass::External).unwrap_err();
        assert_eq!(err.field, "gpu_clock_mhz");
    }

    #[test]
    fn an_external_phone_may_omit_everything() {
        let state = host_state(&HostInput::default(), Platform::Android, DeviceClass::External)
            .unwrap();
        assert!(matches!(state, HostState::Android(ref s) if s.lab.is_none()));
    }

    #[test]
    fn lab_fields_are_all_or_none_on_an_external_phone() {
        let input = HostInput {
            gpu_clock_mhz: Some(980),
            ..Default::default()
        };
        let err = host_state(&input, Platform::Android, DeviceClass::External).unwrap_err();
        assert_eq!(err.field, "bsp_version");
        assert!(err.message.contains("all together"));
    }

    #[test]
    fn an_internal_phone_needs_the_full_snapshot() {
        let err = host_state(&HostInput::default(), Platform::Android, DeviceClass::Internal)
            .unwrap_err();
        assert_eq!(err.field, "bsp_version");
        assert!(err.message.contains("required on an internal android device"));

        let mut input = HostInput {
            bsp_version: Some("bsp".into()),
            sumd_driver_version: Some("sumd".into()),
            gpu_clock_mhz: Some(980),
            mif_clock_mhz: Some(5333),
            int_clock_mhz: Some(934),
            device_uptime_seconds: Some(10),
            battery_charging: Some(false),
            initial_temperature_celsius: Some(30.0),
            max_temperature_celsius: Some(40.0),
            thermal_throttling: Some(false),
            ..Default::default()
        };
        assert!(host_state(&input, Platform::Android, DeviceClass::Internal).is_ok());
        input.thermal_throttling = None;
        let err = host_state(&input, Platform::Android, DeviceClass::Internal).unwrap_err();
        assert_eq!(err.field, "thermal_throttling");
    }

    #[test]
    fn clocks_and_temperatures_are_range_checked() {
        let input = HostInput {
            mif_clock_mhz: Some(0),
            ..Default::default()
        };
        let err = host_state(&input, Platform::Android, DeviceClass::External).unwrap_err();
        assert_eq!(err.field, "mif_clock_mhz");

        let input = HostInput {
            max_temperature_celsius: Some(200.0),
            ..Default::default()
        };
        let err = host_state(&input, Platform::Android, DeviceClass::External).unwrap_err();
        assert_eq!(err.field, "max_temperature_celsius");
    }
}
