use std::io::ErrorKind;

use rppal::pwm::{Channel, Polarity, Pwm};
use sysinfo::{ComponentExt, System, SystemExt};

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

const UDEV_RULES: &str = r#"
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

/// Returns the temperature of the CPU in degrees Celsius.
fn get_cpu_temp(sys: &mut System) -> f32 {
    sys.refresh_components();

    for component in sys.components() {
        println!("{:?}", component);
    }

    let cpu_temp = sys
        .components()
        .iter()
        .map(|s| s.temperature())
        .next()
        .unwrap_or(0.0);

    cpu_temp
}

/// Returns the fan speed as a value between 0.0 and 1.0.
fn handle_fan_speed(cpu_temp: f32, pwm: &mut Pwm) -> Result<(), std::io::Error> {
    let fan_speed = match cpu_temp {
        t if t < OFF_TEMP => FAN_OFF,
        t if t < MIN_TEMP => FAN_LOW,
        t if t < MAX_TEMP => FAN_LOW + ((t - MIN_TEMP).min(MAX_TEMP) * FAN_GAIN),
        _ => FAN_MAX,
    };
    println!("CPU temp: {}°C, fan speed: {}%", cpu_temp, fan_speed);
    pwm.set_duty_cycle(fan_speed as f64)
        .map_err(|rppal::pwm::Error::Io(e)| e)?;
    Ok(())
}

fn main() {
    let mut sys = System::new();

    let mut pwm_pin =
        match Pwm::with_frequency(Channel::Pwm0, PWM_FREQUENCY, 0.0, Polarity::Normal, true) {
            Ok(pin) => pin,
            Err(rppal::pwm::Error::Io(e)) => match e.kind() {
                ErrorKind::PermissionDenied => {
                    eprintln!(
                        "{}",
                        format!(
                            "Make sure /sys/class/pwm and all of its subdirectories are owned by \
                             root:gpio, the current user is a member of the gpio group, and udev \
                             is properly configured as mentioned below. Alternatively, you can \
                             launch your application using sudo.\n\n{}",
                            UDEV_RULES
                        )
                    );
                    return;
                }
                _ => panic!("Error: {}", e),
            },
        };

    loop {
        sys.refresh_components();
        let cpu_temp = get_cpu_temp(&mut sys);
        handle_fan_speed(cpu_temp, &mut pwm_pin).expect("Error setting fan speed");
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}
