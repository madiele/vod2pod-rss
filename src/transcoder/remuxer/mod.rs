use std::io::{Cursor, Write, ErrorKind};

use ebml_iterable::TagWriter;
use log::{debug, error};
use webm_iterable::{WebmIterator, matroska_spec::MatroskaSpec};

pub trait Remuxer<W: Write>: Write {
    fn skip_header(&mut self, bool: bool) -> ();
    fn new(inner_writer: W) -> Self;
    fn get_mut(&mut self) -> &mut W;
}

pub struct EbmlRemuxer<T: Write> {
    pub internal_writer: TagWriter<T>,
    skip_header: bool,
}

impl<T: Write> Write for EbmlRemuxer<T> {

    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let len = buf.len();
        let iter = WebmIterator::new(Cursor::new(buf.to_vec()), &[
            MatroskaSpec::Slices(ebml_iterable::specs::Master::Start),
            MatroskaSpec::Ebml(ebml_iterable::specs::Master::Start),
            MatroskaSpec::Tags(ebml_iterable::specs::Master::Start),
            MatroskaSpec::Tracks(ebml_iterable::specs::Master::Start),
            MatroskaSpec::Info(ebml_iterable::specs::Master::Start),
            MatroskaSpec::SeekHead(ebml_iterable::specs::Master::Start),
            MatroskaSpec::Cluster(ebml_iterable::specs::Master::Start),
        ]);
        for tag in iter {
            if let Ok(tag) = tag {
                //debug!("read tag {:?}", tag);
                //let resx = match tag {
                //    MatroskaSpec::Timestamp(_) => {_ = self.internal_writer.write(&MatroskaSpec::Cluster(Master::Full(vec![tag.clone()])))},
                //    MatroskaSpec::SimpleBlock(_) => continue,
                //    _ => (),
                //};
                //debug!("resx: {:?}", resx);

                if self.skip_header {
                    match tag {
                        MatroskaSpec::Ebml(ebml_iterable::specs::Master::Full(_)) => { continue },
                        MatroskaSpec::Tags(ebml_iterable::specs::Master::Full(_)) => continue,
                        MatroskaSpec::Tracks(ebml_iterable::specs::Master::Full(_)) => continue,
                        MatroskaSpec::Info(ebml_iterable::specs::Master::Full(_)) => continue,
                        MatroskaSpec::SeekHead(ebml_iterable::specs::Master::Full(_)) => continue,
                        MatroskaSpec::Void(_) => continue,
                        _ => (),
                    }
                }
                let res = match self.internal_writer.write(&tag) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        //_ = match e {
                        //    ebml_iterable::error::TagWriterError::UnexpectedTag { tag_id: _, current_path: _ } => self.internal_writer.write(&MatroskaSpec::Cluster(ebml_iterable::specs::Master::End)),
                        //    _ => Ok(()),
                        //};
                        error!("tag: {:?}", e);
                        error!("err: {:?}", e);
                        //self.internal_writer.write_unknown_size(&tag)
                        Err(e)
                    },
                };

                //let res = match self.internal_writer.write(&tag) {
                //    Ok(_) => Ok(()),
                //    Err(e) => {
                //        _ = match e {
                //            ebml_iterable::error::TagWriterError::UnexpectedTag { tag_id: _, current_path: _ } => self.internal_writer.write(&MatroskaSpec::Cluster(ebml_iterable::specs::Master::End)),
                //            _ => Ok(()),
                //        };
                //        error!("tag: {:?}", e);
                //        error!("err: {:?}", e);
                //        self.internal_writer.write_unknown_size(&tag)
                //    },
                //};
                //debug!("written tag: {:?}", tag);
                if res.is_err() {error!("{:?}", res)};
            } else {
                return Err(ErrorKind::BrokenPipe.into())
            }
        }
        Ok(len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.internal_writer.private_flush().map_err(|_e| ErrorKind::BrokenPipe.into())
    }
}

impl<W: Write> Remuxer<W> for EbmlRemuxer<W> {
    fn new(dest_data: W) -> Self {
        let internal_writer = TagWriter::new(dest_data);
        EbmlRemuxer {
            skip_header: false,
            internal_writer,
        }
    }

    fn get_mut(&mut self) -> &mut W {
        self.internal_writer.get_mut()
    }

    fn skip_header(&mut self, value: bool) -> () {
        self.skip_header = value;
    }
}





