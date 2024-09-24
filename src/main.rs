use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use rodio::{decoder, Sink};
use rodio::{Decoder, OutputStream};
use tokio::io::{self, stdin, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream as TokioTcpStream, UdpSocket};
use tokio::sync::RwLock;
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // get handle to new audio stream to default audio device.
    // the handle is a buffer that you can write bytes to.
    let (_stream, stream_handle) = OutputStream::try_default()
        .expect("Could not initialise connection to default audio device.");

    // create a sink that wraps the raw stream handle,
    // allowing for higher level methods like playing, pausing, and appending to the buffer
    let sink = Sink::try_new(&stream_handle).expect("Could not create audio sink");

    sink.pause();

    let mut stream: TokioTcpStream;

    let mut stdin = io::BufReader::new(stdin());

    let mut line = String::new();
    let mut code_confirmation_buffer = [0; 1];
    'try_classroom_code: loop {
        stdin.read_line(&mut line).await?;
        let addr = format!("{}:8080", line.trim());
        let Ok(checked_stream) = TokioTcpStream::connect(&addr).await else {
            // read "classroom code" from stdin, the classroom code is the IP address of the host
            // socket. If it cannot connect, it is not the right socket.
            line.clear();
            continue 'try_classroom_code;
        };

        stream = checked_stream;

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

    let addr = line.clone();
    line.clear();
    // send students name to server
    stdin.read_line(&mut line).await?;
    let name = format!("{}\n", line.trim());
    let _ = stream.write(name.as_bytes()).await?;

    let _ = stream.write(b"send").await?;
    let mut len_bytes = [0; 8];
    stream.read_exact(&mut len_bytes).await?;
    let data_len = u64::from_be_bytes(len_bytes);

    let mut buf = vec![0; data_len as usize];
    stream.read_exact(&mut buf).await?;

    let cursor = Cursor::new(buf);

    //if let Ok(decoder) = Decoder::new(cursor) {
    //    sink.append(decoder);
    //}

    let decoder = Decoder::new(cursor).expect("cannot create decoder from cursor.");
    sink.append(decoder);
    // after sending the header with the data length, and the actual data, the socket will just
    // send the state of the audio at regular intervals, a 0 byte being paused, and a 1 byte being
    // played. this keeps the playback in sync, as even though the host program can handle multiple
    // client connections concurrently, the state is in a unique mutex that is passed to each
    // connection handler
    let stream = UdpSocket::bind("0.0.0.0:0")
        .await
        .expect("Could not bind to udp port");
    stream.connect(&format!("{}:4565", addr.trim())).await?;

    let students = Arc::new(RwLock::default());

    tokio::spawn({
        let students = students.clone();
        async move {
            send_students(students).await;
        }
    });

    do_state(stream, students, sink).await
    //#[allow(unused_labels)]
    //'state_loop: loop {
    //    stream.send(&[1]).await?;
    //    stream.recv(&mut state_buf).await?;
    //    *students.write().await = std::str::from_utf8(&state_buf[1..])?.to_string();
    //    match state_buf[0] {
    //        0 => sink.pause(),
    //        1 => sink.play(),
    //        _ => {
    //            panic!("Server should send 0 or 1 bytes at this point in time, got {state_buf:?}")
    //        }
    //    }
    //    time::sleep(Duration::from_micros(200)).await;
    //}
}

async fn do_state(
    stream: UdpSocket,
    students: Arc<RwLock<String>>,
    sink: Sink,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut state_buf = [0; 1024];
    loop {
        stream.send(&[1]).await?;
        stream.recv(&mut state_buf).await?;
        *students.write().await = std::str::from_utf8(&state_buf[1..])?.to_string();
        match state_buf[0] {
            0 => sink.pause(),
            1 => sink.play(),
            _ => {
                panic!("Server should send 0 or 1 bytes at this point in time, got {state_buf:?}")
            }
        }
        time::sleep(Duration::from_micros(200)).await;
    }
}

async fn send_students(students: Arc<RwLock<String>>) {
    let mut line = String::new();
    let mut stdin = io::BufReader::new(stdin());

    loop {
        let _ = stdin.read_line(&mut line).await;
        if line.as_str().trim() == "students" {
            println!("{}", students.read().await)
        }
        line.clear();
    }
}
