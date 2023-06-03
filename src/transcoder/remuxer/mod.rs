use std::io::{Cursor, Write, ErrorKind};

use ebml_iterable::TagWriter;
use log::debug;
use webm_iterable::WebmIterator;

pub trait Remuxer<W: Write>: Write {
    fn new(inner_writer: W) -> Self;
    fn get_mut(&mut self) -> &mut W;
}

pub struct EbmlRemuxer<T: Write> {
    pub internal_writer: TagWriter<T>,
}

impl<T: Write> Write for EbmlRemuxer<T> {

    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let len = buf.len();
        let iter = WebmIterator::new(Cursor::new(buf.to_vec()), &[]);
        for tag in iter {
            if let Ok(tag) = tag {
                //debug!("read tag {:?}", tag);
                //let resx = match tag {
                //    MatroskaSpec::Timestamp(_) => {_ = self.internal_writer.write(&MatroskaSpec::Cluster(Master::Full(vec![tag.clone()])))},
                //    MatroskaSpec::SimpleBlock(_) => continue,
                //    _ => (),
                //};
                //debug!("resx: {:?}", resx);
                let res = self.internal_writer.write(&tag);
                debug!("res1: {:?}", tag);
                if res.is_err() { panic!() };
            } else {
                return Err(ErrorKind::BrokenPipe.into())
            }
        }
        Ok(len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.internal_writer.flush().map_err(|_e| ErrorKind::BrokenPipe.into())
    }
}

impl<W: Write> Remuxer<W> for EbmlRemuxer<W> {
    fn new(dest_data: W) -> Self {
        let internal_writer = TagWriter::new(dest_data);
        EbmlRemuxer {
            internal_writer,
        }
    }

    fn get_mut(&mut self) -> &mut W {
        self.internal_writer.get_mut()
    }
}





