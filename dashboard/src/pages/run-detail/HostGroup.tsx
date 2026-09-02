import { FieldGroup } from "../../components/FieldGroup";
import { DeviceClassBadge, ThrottledBadge } from "../../components/Status";
import { formatBytes, formatDurationSeconds } from "../../lib/format";
import { type Run, celsius, labValue, mhz, yesNo } from "./shared";

/** The host snapshot: device state and pinned clocks for an Android run,
 * one machine description for a Linux run. */
export function HostGroup({ run }: { run: Run }) {
  return run.platform === "android" ? <AndroidDeviceGroups run={run} /> : <LinuxHostGroup run={run} />;
}

function AndroidDeviceGroups({ run }: { run: Run }) {
  const external = run.device_class === "external";
  const lab = labValue(external);
  return (
    <>
      <FieldGroup
        title="Device state"
        note={external ? "external device — no BSP, driver or thermal capture" : undefined}
        fields={[
          { label: "Platform", value: run.platform, mono: true },
          { label: "Device class", value: <DeviceClassBadge deviceClass={run.device_class} /> },
          { label: "Device serial", value: run.device_serial, mono: true },
          { label: "Device model", value: run.device_model, mono: true },
          { label: "OS", value: run.host_os },
          { label: "Kernel", value: run.host_kernel, mono: true },
          { label: "SoC", value: run.host_cpu_model, mono: true, hint: "Reported as the host CPU model." },
          { label: "CPU count", value: run.host_cpu_count == null ? null : String(run.host_cpu_count), mono: true },
          {
            label: "Memory",
            value: run.host_memory_bytes == null ? null : formatBytes(run.host_memory_bytes),
            mono: true,
          },
          { label: "GPU", value: run.host_accelerator },
          { label: "GPU driver", value: run.host_accelerator_driver, mono: true },
          { label: "BSP version", value: lab(run.bsp_version), mono: true },
          { label: "SUMD driver", value: lab(run.sumd_driver_version), mono: true },
          {
            label: "Uptime",
            value: lab(run.device_uptime_seconds == null ? null : formatDurationSeconds(run.device_uptime_seconds)),
          },
          { label: "Battery charging", value: lab(yesNo(run.battery_charging)) },
          { label: "Initial temp.", value: lab(celsius(run.initial_temperature_celsius)) },
          { label: "Max temp.", value: lab(celsius(run.max_temperature_celsius)) },
          {
            label: "Throttling",
            value: run.thermal_throttling ? <ThrottledBadge /> : lab(yesNo(run.thermal_throttling)),
          },
        ]}
      />
      <FieldGroup
        title="Performance configuration"
        note={external ? "clocks are not pinnable on an external device" : "pinned clocks"}
        fields={[
          { label: "GPU clock", value: lab(mhz(run.gpu_clock_mhz)), mono: true },
          { label: "MIF clock", value: lab(mhz(run.mif_clock_mhz)), mono: true },
          { label: "INT clock", value: lab(mhz(run.int_clock_mhz)), mono: true },
        ]}
      />
    </>
  );
}

function LinuxHostGroup({ run }: { run: Run }) {
  return (
    <FieldGroup
      title="Host"
      fields={[
        { label: "Platform", value: run.platform, mono: true },
        { label: "Device class", value: <DeviceClassBadge deviceClass={run.device_class} /> },
        { label: "Hostname", value: run.device_serial, mono: true },
        { label: "Machine", value: run.device_model, mono: true },
        { label: "OS", value: run.host_os },
        { label: "Kernel", value: run.host_kernel, mono: true },
        { label: "CPU", value: run.host_cpu_model },
        { label: "CPU count", value: run.host_cpu_count == null ? null : String(run.host_cpu_count), mono: true },
        { label: "Memory", value: run.host_memory_bytes == null ? null : formatBytes(run.host_memory_bytes), mono: true },
        {
          label: "Accelerator",
          value: run.host_accelerator ? <span title={run.host_accelerator}>{run.host_accelerator}</span> : null,
        },
        { label: "Accel. driver", value: run.host_accelerator_driver, mono: true },
        {
          label: "Uptime",
          value: run.device_uptime_seconds == null ? null : formatDurationSeconds(run.device_uptime_seconds),
        },
        {
          label: "Throttling",
          value: run.thermal_throttling ? <ThrottledBadge /> : yesNo(run.thermal_throttling),
        },
      ]}
    />
  );
}
