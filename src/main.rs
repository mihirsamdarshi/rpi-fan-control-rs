use std::{f32::consts::PI, io::ErrorKind};

use rppal::pwm::{Channel, Polarity, Pwm};

const PWM_FREQUENCY: f64 = 25_000.0;
/// [°C] temperature below which to stop the fan
const OFF_TEMP: f32 = 40.0;
/// [°C] temperature above which to start the fan
const MIN_TEMP: f32 = 45.0;
/// [°C] temperature above which to start the fan at full speed
const MAX_TEMP: f32 = 75.0;

const FAN_OFF: f32 = 0.0;
const FAN_LOW: f32 = 0.1;
const FAN_MAX: f32 = 1.0;
const FAN_GAIN: f32 = (FAN_MAX - FAN_LOW) / (MAX_TEMP - MIN_TEMP);

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

const PERMISSION_ERROR: &str = r#"
By default, both channels are disabled.

To enable only PWM0 on its default pin (BCM GPIO 18, physical pin 12), add dtoverlay=pwm to /boot/config.txt on Raspberry Pi OS or boot/firmware/usercfg.txt on Ubuntu.
If you need both PWM channels, replace pwm with pwm-2chan, which enables PWM0 on BCM GPIO 18 (physical pin 12), and PWM1 on BCM GPIO 19 (physical pin 35).
More details on enabling and configuring PWM on other GPIO pins than the default ones can be found in /boot/overlays/README.
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

fn fan_curve(temp: f32) -> f32 {
    (0.5 * (1.0 - ((PI * temp) / 50.0).sin())
        + (FAN_LOW + ((temp - MIN_TEMP).min(MAX_TEMP) * FAN_GAIN)))
        / 2.0
}

/// Returns the fan speed as a value between 0.0 and 1.0.
fn handle_fan_speed(cpu_temp: f32, pwm: &mut Pwm) -> Result<(), std::io::Error> {
    let fan_speed = match cpu_temp {
        t if t < OFF_TEMP => FAN_OFF,
        t if t < MIN_TEMP => FAN_LOW,
        t if t < MAX_TEMP => fan_curve(t),
        _ => FAN_MAX,
    };
    println!("CPU temp: {}°C, fan speed: {}%", cpu_temp, fan_speed);
    pwm.set_duty_cycle(fan_speed as f64)
        .map_err(|rppal::pwm::Error::Io(e)| e)?;
    Ok(())
}

fn main() {
    let mut pwm_pin =
        match Pwm::with_frequency(Channel::Pwm0, PWM_FREQUENCY, 0.0, Polarity::Normal, true) {
            Ok(pin) => pin,
            Err(rppal::pwm::Error::Io(e)) => match e.kind() {
                ErrorKind::PermissionDenied => {
                    eprintln!(
                        "{}\n\n{}",
                        "Make sure /sys/class/pwm and all of its subdirectories are owned by \
                         root:gpio, the current user is a member of the gpio group, and udev is \
                         properly configured as mentioned below. Alternatively, you can launch \
                         your application using sudo.",
                        UDEV_ERROR
                    );
                    std::process::exit(1);
                }
                ErrorKind::NotFound => {
                    eprintln!(
                        "{}\n\n{}",
                        "You may have forgotten to enable the selected PWM channel. The \
                         configuration options to enable either of the two PWM channels are \
                         listed below.",
                        PERMISSION_ERROR
                    );
                    std::process::exit(1);
                }
                _ => panic!("Error: {}", e),
            },
        };

    loop {
        let cpu_temp = get_cpu_temp();
        handle_fan_speed(cpu_temp, &mut pwm_pin).expect("Error setting fan speed");
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}
