//! Simple example of using remote_xy library with only one output signal plotted in the app.
//! Just run this example and add a new Ethernet device in the RemoteXY app (port 6377)

use anyhow::Result;
use remote_xy::remote_xy;
use remote_xy::RemoteXY;
use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;

const CONF_BUF: &[u8] = &[
    255, 0, 0, 4, 0, 11, 0, 16, 31, 0, 68, 17, 0, 0, 100, 63, 8, 36,
];

#[derive(Serialize, Deserialize, Default)]
#[repr(C)]
struct InputData {
    // no input variables
}

#[derive(Serialize, Deserialize)]
#[repr(C)]
struct OutputData {
    // output variables
    online_graph_1: f32,
    // do not include the `connect_flag` variable
}

// provide custom initial values for the output variables if needed:
impl Default for OutputData {
    fn default() -> Self {
        Self {
            online_graph_1: 1.23,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let remotexy = remote_xy!(InputData, OutputData, "[::]:6377", CONF_BUF).await?;
    let mut output = OutputData::default();
    let mut x = 0.0;

    while remotexy.is_connected() == false {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    println!("Connected to RemoteXY app");

    // wait some time to see the effect of default custum signals
    tokio::time::sleep(Duration::from_millis(2000)).await;

    println!("Starting to send data");
    loop {
        output.online_graph_1 = f32::sin(x);
        remotexy.set_output(&output);
        x += 0.1;

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
