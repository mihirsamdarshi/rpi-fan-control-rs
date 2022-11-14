use std::{
    f32::consts::PI,
    io::ErrorKind,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use once_cell::sync::Lazy;
use rppal::{
    gpio::{Gpio, Trigger},
    pwm::{Channel, Polarity, Pwm},
};

/// The PWM frequency that the PWM fan should operate at (for the Noctua A4x10)
const PWM_FREQUENCY: f64 = 25_000.0;
/// [°C] temperature below which to stop the fan
const OFF_TEMP: f32 = 40.0;
/// [°C] temperature above which to start the fan
const MIN_TEMP: f32 = 45.0;
/// [°C] temperature above which to start the fan at full speed
const MAX_TEMP: f32 = 75.0;

/// The speed (percentage) that the fan is off at
const FAN_OFF: f32 = 0.0;
/// The speed (percentage) that the lowest setting of the fan should be
const FAN_LOW: f32 = 0.1;
/// The speed (percentage) that the max setting of the fan is
const FAN_MAX: f32 = 1.0;
/// The steps that the fan speed should increase per each degree that the
/// temperature increase
const FAN_GAIN: f32 = (FAN_MAX - FAN_LOW) / (MAX_TEMP - MIN_TEMP);
/// The number of GPIO pulses per revolution - Noctua fans puts out two pluses
/// per revolution
const FAN_PULSE: f32 = 2.0;

const UDEV_ERROR: &str = r#"
As of kernel version 4.14.34, released on April 16 2018, it's possible to configure your Raspberry Pi to allow non-root access to PWM. 
4.14.34 includes a patch that allows udev to change file permissions when a PWM channel is exported. 
This will let any user that is a member of the GPIO group configure PWM without having to use sudo.

The udev rules needed to make this work haven't been patched in yet as of June 2018, but you can easily add them yourself. 
Make sure you're running 4.14.34 or later, and append the following snippet to /etc/udev/rules.d/99-com.rules. Reboot the Raspberry Pi afterwards.

```
SUBSYSTEM=="pwm*", PROGRAM="/bin/sh -c '\
    chown -R root:gpio /sys/class/pwm && chmod -R 770 /sys/class/pwm;\
    chown -R root:gpio /sys/devices/platform/soc/*.pwm/pwm/pwmchip* &&\
    chmod -R 770 /sys/devices/platform/soc/*.pwm/pwm/pwmchip*\
'"
```
"#;

const PWM_PERMISSION_ERROR: &str = r#"
By default, both channels are disabled.

To enable only PWM0 on its default pin (BCM GPIO 18, physical pin 12), add dtoverlay=pwm to /boot/config.txt on Raspberry Pi OS or boot/firmware/usercfg.txt on Ubuntu.
If you need both PWM channels, replace pwm with pwm-2chan, which enables PWM0 on BCM GPIO 18 (physical pin 12), and PWM1 on BCM GPIO 19 (physical pin 35).
More details on enabling and configuring PWM on other GPIO pins than the default ones can be found in /boot/overlays/README.
"#;

const GPIO_PERMISSION_ERROR: &str = r#"
In recent releases of Raspberry Pi OS (December 2017 or later), users that are part of the gpio group (like the default pi user) can access /dev/gpiomem and /dev/gpiochipN (N = 0-2) without needing additional permissions. 
Either the current user isn’t a member of the gpio group, or your Raspberry Pi OS distribution isn't up-to-date and doesn't automatically configure permissions for the above-mentioned files. 
Updating Raspberry Pi OS to the latest release should fix any permission issues. 
Alternatively, although not recommended, you can run your application with superuser privileges by using sudo.

If you’re unable to update Raspberry Pi OS and its packages (namely raspberrypi-sys-mods) to the latest available release, or updating hasn't fixed the issue, you might be able to manually update your udev rules to set the appropriate permissions. 
More information can be found at https://github.com/raspberrypi/linux/issues/1225 and https://github.com/raspberrypi/linux/issues/2289.
"#;

/// Returns the temperature of the CPU in degrees Celsius.
fn get_cpu_temp() -> f32 {
    let temp_unparsed = match std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
        Ok(temp) => temp,
        Err(e) => match e.kind() {
            ErrorKind::PermissionDenied => {
                panic!("Failed to read /sys/class/thermal/thermal_zone0/temp.")
            }
            ErrorKind::NotFound => {
                panic!("No temperature sensor found. Make sure you're running on a Raspberry Pi.")
            }
            _ => "45000".to_string(),
        },
    };
    temp_unparsed.trim().parse::<f32>().unwrap_or(45000.0) / 1000.0
}

/// The custom fan curve that determines the speed that the fan should be at
/// based on the temperature reported back by the Raspberry Pi
#[inline]
fn fan_curve(temp: f32) -> f32 {
    (0.5 * (1.0 - ((PI * temp) / 50.0).sin())
        + (FAN_LOW + ((temp - MIN_TEMP).min(MAX_TEMP) * FAN_GAIN)))
        / 2.0
}

/// Returns the fan speed as a value between 0.0 and 1.0.
fn handle_fan_speed(cpu_temp: f32, pwm: &mut Pwm) -> Result<f32, std::io::Error> {
    let fan_percentage = match cpu_temp {
        t if t < OFF_TEMP => FAN_OFF,
        t if t < MIN_TEMP => FAN_LOW,
        t if t < MAX_TEMP => fan_curve(t),
        _ => FAN_MAX,
    };
    pwm.set_duty_cycle(f64::from(fan_percentage))
        .map_err(|rppal::pwm::Error::Io(e)| e)?;
    Ok(fan_percentage * 100.0)
}

static TIME_DIFF: Lazy<Arc<Mutex<Instant>>> = Lazy::new(|| Arc::new(Mutex::new(Instant::now())));
static RPM: Lazy<Arc<Mutex<Vec<f32>>>> = Lazy::new(|| Arc::new(Mutex::new(vec![0.0])));

fn main() {
    let mut pwm_pin =
        match Pwm::with_frequency(Channel::Pwm0, PWM_FREQUENCY, 0.0, Polarity::Normal, true) {
            Ok(pin) => pin,
            Err(rppal::pwm::Error::Io(e)) => match e.kind() {
                ErrorKind::PermissionDenied => {
                    eprintln!(
                        "Make sure /sys/class/pwm and all of its subdirectories are owned by \
                         root:gpio, the current user is a member of the gpio group, and udev is \
                         properly configured as mentioned below. Alternatively, you can launch \
                         your application using sudo.\n\n{}",
                        UDEV_ERROR
                    );
                    std::process::exit(1);
                }
                ErrorKind::NotFound => {
                    eprintln!(
                        "You may have forgotten to enable the selected PWM channel. The \
                         configuration options to enable either of the two PWM channels are \
                         listed below.\n\n{}",
                        PWM_PERMISSION_ERROR
                    );
                    std::process::exit(1);
                }
                _ => panic!("Error: {e}"),
            },
        };

    let gpio = Gpio::new().unwrap();
    let mut fan_speed_pin = match gpio.get(24) {
        Ok(p) => p,
        Err(e) => match e {
            rppal::gpio::Error::Io(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
            rppal::gpio::Error::PermissionDenied(e) => {
                eprintln!("Error: {e}\n\n{GPIO_PERMISSION_ERROR}");
                std::process::exit(1);
            }
            _ => panic!("Error: {e}"),
        },
    }.into_input_pullup();

    fan_speed_pin
        .set_async_interrupt(Trigger::FallingEdge, |_| {
            let mut time_diff = TIME_DIFF.lock().unwrap();
            let dt = Instant::now() - time_diff.clone();

            if dt < Duration::from_millis(5) {
                return;
            }

            let freq: f32 = 1.0 / dt.as_secs_f32();
            let rpm = (freq / FAN_PULSE) * 60.0;
            let mut rpm_guard = RPM.lock().unwrap();
            (*rpm_guard).push(rpm);
            *time_diff = Instant::now();
        })
        .unwrap();

    loop {
        let cpu_temp = get_cpu_temp();
        let fan_percentage =
            handle_fan_speed(cpu_temp, &mut pwm_pin).expect("Error setting fan speed");
        let mut rpm_guard = RPM.lock().unwrap();
        let avg_rpm = rpm_guard.drain(..).reduce(|acc, x| acc + x).unwrap_or(0.0) / rpm_guard.len() as f32;
        println!(
            "CPU Temp: {cpu_temp:.2}°C, Fan Percentage: {fan_percentage:.2}%, Fan Speed: \
             {avg_rpm:.2} RPM",
        );
        *rpm_guard = Vec::new();
        std::thread::sleep(Duration::from_secs(5));
    }
}
