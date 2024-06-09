use std::io::Cursor;
use std::time::Duration;

use rodio::Sink;
use rodio::{Decoder, OutputStream};
use tokio::io::{self, stdin, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream as TokioTcpStream, UdpSocket};
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (_stream, stream_handle) = match OutputStream::try_default() {
        // get handle to new audio stream to default audio device.
        // the handle is a buffer that you can write bytes to.
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            panic!("Could not initialise connection to default audio device.");
        }
    };

    let sink = Sink::try_new(&stream_handle)?;
    // create a sink that wraps the raw stream handle,
    // allowing for higher level methods like playing, pausing, and appending to the buffer
    sink.pause();

    let mut stream: TokioTcpStream;

    let mut stdin = io::BufReader::new(stdin());

    let mut line = String::new();
    let mut code_confirmation_buffer = [0; 1];
    'try_classroom_code: loop {
        stdin.read_line(&mut line).await?;
        let addr = format!("{}:8080", line.trim());
        stream = match TokioTcpStream::connect(&addr).await {
            // read "classroom code" from stdin, the classroom code is the IP address of the host
            // socket. If it cannot connect, it is not the right socket.
            Ok(o) => o,
            Err(_e) => {
                println!("{addr}");
                line.clear();
                continue 'try_classroom_code;
            }
        };

        // write the code from stdin into the socket at that address, if it gets the expected
        // response, it is deifnitely the right socket, and if not, it connected to random socket
        // that was open
        let _ = stream.write(line.as_bytes()).await?;
        stream.read_exact(&mut code_confirmation_buffer).await?;
        if code_confirmation_buffer == [1] {
            println!("correct");
            break 'try_classroom_code;
        }
        line.clear();
        println!("incorrect");
    }

    let _ = stream.write(b"send").await?;
    let mut len_bytes = [0; 8];
    stream.read_exact(&mut len_bytes).await?;
    let data_len = u64::from_be_bytes(len_bytes);

    let mut buf = vec![0; data_len as usize];
    stream.read_exact(&mut buf).await?;

    let cursor = Cursor::new(buf);

    if let Ok(decoder) = Decoder::new(cursor) {
        sink.append(decoder);
    }

    // after sending the header with the data length, and the actual data, the socket will just
    // send the state of the audio at regular intervals, a 0 byte being paused, and a 1 byte being
    // played. this keeps the playback in sync, as even though the host program can handle multiple
    // client connections concurrently, the state is in a unique mutex that is passed to each
    // connection handler
    let mut state_buf = [0; 1];
    let stream = UdpSocket::bind("0.0.0.0:8000").await?;
    stream.connect(&format!("{}:4565", line.trim())).await?;
    #[allow(unused_labels)]
    'state_loop: loop {
        stream.recv(&mut state_buf).await?;
        match state_buf {
            [0] => sink.pause(),
            [1] => sink.play(),
            _ => {
                panic!("Server sent unknown command")
            }
        }
        time::sleep(Duration::from_micros(500)).await;
    }
}
