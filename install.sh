#!/bin/sh

# Install the package

cargo build -Zbuild-std --release
cp target/aarch64-unknown-linux/release/rpi_fan_control /usr/sbin/ /usr/local/sbin/rpi_fan_control
cp rpi_fan_control.service /etc/systemd/system/rpi_fan_control.service
systemctl enable rpi_fan_control.service
systemctl start rpi_fan_control.service
