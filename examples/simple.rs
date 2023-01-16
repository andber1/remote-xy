//! Simple example of using remote_xy library
//! Just run this example and add a new Ethernet device in the RemoteXY app (port 6377)
//! Changing the slider and switch on the app will change circular bar and LED.

use anyhow::Result;
use remote_xy::remote_xy;
use remote_xy::RemoteXY;
use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;

const CONF_BUF: &[u8] = &[
    255, 2, 0, 2, 0, 59, 0, 16, 31, 1, 4, 0, 44, 10, 10, 78, 2, 26, 2, 0, 9, 77, 22, 11, 2, 26, 31,
    31, 79, 78, 0, 79, 70, 70, 0, 72, 12, 9, 16, 23, 23, 2, 26, 140, 38, 0, 0, 0, 0, 0, 0, 200, 66,
    0, 0, 0, 0, 70, 16, 16, 63, 9, 9, 26, 37, 0,
];

#[derive(Serialize, Deserialize, Debug, Default)]
#[repr(C)]
struct InputData {
    // input variables
    slider_1: i8, // =0..100 slider position
    switch_1: u8, // =1 if switch ON and =0 if OFF
}

#[derive(Serialize, Deserialize, Debug, Default)]
#[repr(C)]
struct OutputData {
    // output variables
    circularbar_1: i8, // from 0 to 100
    led_1: u8,         // led state 0 .. 1
                       // do not include the `connect_flag` variable
}

#[tokio::main]
async fn main() -> Result<()> {
    let remotexy = remote_xy!(InputData, OutputData, "[::]:6377", CONF_BUF).await?;
    let mut output = OutputData::default();

    loop {
        let input = remotexy.get_input();
        println!("Received input: {:?}", input);

        output.circularbar_1 = input.slider_1;
        output.led_1 = input.switch_1;
        remotexy.set_output(&output);

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
