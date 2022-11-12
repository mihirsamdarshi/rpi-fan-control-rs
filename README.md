# Raspberry Pi Fan Control

A simple utility written in Rust to help control the fan speed of a Raspberry Pi using 
hardware PWM to avoid the software-based PWM that leads to high CPU usage

I implemented a simple fan curve, seen below.

![Graph of the fan curve](img/curve.png)
