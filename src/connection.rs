use std::io::{self, Cursor};

use bytes::{Buf,BytesMut};
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use mini_redis::{Frame, Result};
use mini_redis::frame::Error::Incomplete;

pub struct Connection {
    stream: BufWriter<TcpStream>,
    buffer: BytesMut,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Connection {
        Connection {
            stream: BufWriter::new(stream),
            // Allocate the buffer with 4kb of capacity.
            buffer: BytesMut::with_capacity(4096),
        }
    }

    pub async fn read_frame(&mut self) -> Result<Option<Frame>> {
       loop {
           // Attempt to parse a frame from the buffered data.
           // If enough data has been buffered, the frame is returned.
           if let Some(frame) = self.parse_frame()? {
               return Ok(Some(frame));
           }

           // There is not enough buffered data to read a frame.
           // Attempt to read more data from the socket.
           //
           // On success, the number of bytes is returned.
           // 0 indicates "end of stream".
            if 0 == self.stream.read_buf(&mut self.buffer).await? {
               if self.buffer.is_empty() {
                   return Ok(None);
               } else {
                   return Err("connection reset by peer".into());
               }
            }
       }
    }

    fn parse_frame(&mut self) -> Result<Option<Frame>> {
        // Create the T: Buf type.
        let mut buf = Cursor::new(self.buffer.as_ref());

        match Frame::check(&mut buf) {
            Ok(_) => {
                // Frame::check set cursor position at end of frame.
               let len = buf.position() as usize;

                // Reset the internal cursor for the call to parse.
                buf.set_position(0);

                // Parse the frame.
                let frame = Frame::parse(&mut buf)?;

                // Discard the frame from the buffer.
                self.buffer.advance(len);

                // Return the frame to the caller.
                Ok(Some(frame))
            }
            // Not enough data has been buffered.
            Err(Incomplete) => Ok(None),

            // An error was encountered.
            Err(e) => Err(e.into()),
        }
    }

    pub async fn write_frame(&mut self, frame: &Frame) -> io::Result<()> {
        match frame {
            Frame::Array(val) => {
                // Encode the frame type prefix. For an array, it is `*`.
                self.stream.write_u8(b'*').await?;

                // Encode the length of the array.
                self.write_decimal(val.len() as u64).await?;

                // Iterate and encode each entry in the array.
                for entry in &**val {
                    self.write_value(entry).await?;
                }
            }
            _ => self.write_value(frame).await?
        }

        // Ensure the encoded frame is written to the socket.
        // The calls above are to the buffered stream and writes.
        // Calling `flush` writes the remaining contents of the buffer to the socket.
        self.stream.flush().await
    }

    async fn write_value(&mut self, frame: &Frame) -> io::Result<()> {
        const DELIMITER: &[u8] = b"\r\n";

        match frame {
            Frame::Simple(val) => {
                self.stream.write_u8(b'+').await?;
                self.stream.write_all(val.as_bytes()).await?;
                self.stream.write_all(DELIMITER).await?;
            }
            Frame::Error(val) => {
                self.stream.write_u8(b'-').await?;
                self.stream.write_all(val.as_bytes()).await?;
                self.stream.write_all(DELIMITER).await?;
            }
            Frame::Integer(val) => {
                self.stream.write_u8(b':').await?;
                self.write_decimal(*val).await?;
            }
            Frame::Null => {
                self.stream.write_all(b"$-1\r\n").await?;
            }
            Frame::Bulk(val) => {
                let len = val.len();

                self.stream.write_u8(b'$').await?;
                self.write_decimal(len as u64).await?;
                self.stream.write_all(val).await?;
                self.stream.write_all(DELIMITER).await?;
            }

            // Encoding an `Array` from within a value cannot be done using a
            // recursive strategy. In general, async fns do not support
            // recursion. Mini-redis has not needed to encode nested arrays yet,
            // so for now it is skipped.
            Frame::Array(_val) => unimplemented!(),
        }

        self.stream.flush().await?;

        Ok(())
    }

    async fn write_decimal(&mut self, val: u64) -> io::Result<()> {
        use std::io::Write;

        let mut buf = [0u8; 12];
        let mut buf = Cursor::new(buf.as_mut());
        write!(&mut buf, "{}", val)?;

        let pos = buf.position() as usize;
        self.stream.write_all(&buf.get_ref()[..pos]).await?;
        self.stream.write_all(b"\r\n").await?;

        Ok(())
    }
}
