#!/bin/bash

service_exists() {
  local n
  n=$1
  if [[ $(systemctl list-units --all -t service --full --no-legend "$n.service" | sed 's/^\s*//g' | cut -f1 -d' ') == $n.service ]]; then
      return 0
  else
      return 1
    fi
}

cargo build -Zbuild-std --release

if service_exists rpi_fan_control; then
  sudo systemctl stop rpi_fan_control
fi
# Install the package
sudo cp target/aarch64-unknown-linux-gnu/release/rpi_fan_control /usr/local/sbin/
sudo cp lib/rpi_fan_control.service /etc/systemd/system/rpi_fan_control.service

cargo build -Zbuild-std --release

if service_exists rpi_fan_control; then
  sudo systemctl daemon-reload
  sudo systemctl restart rpi_fan_control
else
  sudo systemctl enable rpi_fan_control.service
  sudo systemctl start rpi_fan_control.service
fi