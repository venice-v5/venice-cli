use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, stdin, stdout},
    time::sleep,
};
use vex_v5_serial::{Connection, serial::SerialConnection};

use crate::errors::CliError;

// actual return type should be Result<!, CliError>, but never (!) type is unstable
pub async fn terminal(conn: &mut SerialConnection) -> Result<(), CliError> {
    let mut stdin = stdin();
    let mut stdout = stdout();

    let mut input_buf = [0; 1024];
    let mut output_buf = [0; 1024];

    loop {
        tokio::select! {
            input_size = stdin.read(&mut input_buf) => {
                if let Ok(size) = input_size {
                    conn.write_user(&input_buf[..size]).await?;
                }
            }
            output_size = conn.read_user(&mut output_buf) => {
                if let Ok(size) = output_size {
                    stdout.write_all(&output_buf[..size]).await?;
                }
            }
        }

        sleep(Duration::from_millis(10)).await;
    }
}
