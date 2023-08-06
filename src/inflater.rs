// https://github.com/twilight-rs/twilight/blob/main/gateway/src/shard/processor/compression/inflater.rs

// ISC License (ISC)
//
// Copyright (c) 2019 (c) The Twilight Contributors
//
// Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is
// hereby granted, provided that the above copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS
// SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL
// THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE
// OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.




use flate2::{Decompress, DecompressError, FlushDecompress};
use std::{convert::TryInto,  time::Instant};

const ZLIB_SUFFIX: [u8; 4] = [0x00, 0x00, 0xff, 0xff];
const INTERNAL_BUFFER_SIZE: usize = 32 * 1024;

#[derive(Debug)]
pub struct Inflater {
    decompress: Decompress,
    compressed: Vec<u8>,
    internal_buffer: Vec<u8>,
    buffer: Vec<u8>,
    last_resize: Instant,
}

impl Inflater {
    /// Create a new inflater for a shard.
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(INTERNAL_BUFFER_SIZE),
            compressed: Vec::new(),
            decompress: Decompress::new(true),
            internal_buffer: Vec::with_capacity(INTERNAL_BUFFER_SIZE),
            last_resize: Instant::now(),
        }
    }


    /// Extend the internal compressed buffer with bytes.
    pub fn extend(&mut self, slice: &[u8]) {
        self.compressed.extend_from_slice(&slice);
    }

    /// Decompress the next message if a complete payload was received.
    ///
    /// Returns `None` if an incomplete payload was received.
    ///
    /// # Errors
    ///
    /// This returns `flate2`'s `DecompressError` as its method's type signature
    /// indicates it can return an error, however in reality in versions up to
    /// 1.0.17 it won't.s
    pub fn msg(&mut self) -> Result<Option<&mut [u8]>, DecompressError> {
        let length = self.compressed.len();

        // Check if a partial payload was received. If it was, we can just
        // return that no decompressed message is available.
        if length < 4 || self.compressed[(length - 4)..] != ZLIB_SUFFIX {
            return Ok(None);
        }

        let before = self.decompress.total_in();
        let mut offset = 0;

        loop {
            self.internal_buffer.clear();

            self.decompress.decompress_vec(
                &self.compressed[offset..],
                &mut self.internal_buffer,
                FlushDecompress::Sync,
            )?;

            offset = (self.decompress.total_in() - before)
                .try_into()
                .unwrap_or_default();
            self.buffer.extend_from_slice(&self.internal_buffer[..]);

            let not_at_capacity = self.internal_buffer.len() < self.internal_buffer.capacity();

            if not_at_capacity || offset > self.compressed.len() {
                break;
            }
        }

        self.compressed.clear();

        Ok(Some(&mut self.buffer))
    }

    /// Clear the buffer and shrink it if the capacity is too large.
    ///
    /// If the capacity is 4 times larger than the buffer length then the
    /// capacity will be shrunk to the length.
    pub fn clear(&mut self) {
        self.shrink();

        self.compressed.clear();
        self.internal_buffer.clear();
        self.buffer.clear();
    }



    /// Shrink the capacity of the compressed buffer and payload buffer if at
    /// least 60 seconds have passed since the last shrink.
    fn shrink(&mut self) {
        if self.last_resize.elapsed().as_secs() < 60 {
            return;
        }

        self.compressed.shrink_to_fit();
        self.buffer.shrink_to_fit();


        self.last_resize = Instant::now();
    }
}