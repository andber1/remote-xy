# remote-xy

Control and monitor your Rust application from your smartphone via a graphical interface (based on [RemoteXY](https://remotexy.com/)).

The library will start a server to which the [RemoteXY app](https://remotexy.com/en/download/) can connect. You can use it to control and monitor your application from your smartphone via a graphical interface you create yourself. Connection is established via TCP. This is an alternative to the original [Arduino library](https://github.com/RemoteXY/RemoteXY-Arduino-library) and runs for instance on a Raspberry Pi.

For a quick demo run one of the provided examples (e.g. `cargo run --example simple`), start the RemoteXY app and connect to the device where the example is running (new Ethernet device, port 6377).

For your own graphical interface designs use the [RemoteXY online editor](https://remotexy.com/en/editor/) and generate C-code. Port the provided C-struct and config array to Rust and you are done. See examples and documentation for more details.

Tested with Android RemoteXY app 4.11.9, API version 28.

Licensed under the GPLv2.