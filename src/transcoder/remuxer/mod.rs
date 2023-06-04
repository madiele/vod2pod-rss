use std::io::{Cursor, Write, ErrorKind};

use ebml_iterable::{TagWriter, specs::Master};
use log::{debug, error};
use webm_iterable::{WebmIterator, matroska_spec::MatroskaSpec};

pub trait Remuxer<W: Write>: Write {
    fn skip_header(&mut self, bool: bool) -> ();
    fn new(inner_writer: W) -> Self;
    fn get_mut(&mut self) -> &mut W;
    fn write_padding(&mut self, byte_count: usize) -> std::io::Result<()>;
}

pub struct EbmlRemuxer<T: Write> {
    pub internal_writer: TagWriter<T>,
    skip_header: bool,
}

impl<T: Write> Write for EbmlRemuxer<T> {

    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let len = buf.len(); //FIX: this is wrong, the buff size of written can be different
        let iter = WebmIterator::new(Cursor::new(buf.to_vec()), &[
            //read theese tags as a single Full block instead of piece by piece
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

                if self.skip_header {
                    match tag {
                        MatroskaSpec::Ebml(ebml_iterable::specs::Master::Full(_)) => { debug!("skipping Ebml"); continue },
                        MatroskaSpec::Tags(ebml_iterable::specs::Master::Full(_)) => { debug!("skipping Tags"); continue },
                        MatroskaSpec::Tracks(ebml_iterable::specs::Master::Full(_)) => { debug!("skipping Tracks"); continue },
                        MatroskaSpec::Info(ebml_iterable::specs::Master::Full(_)) => { debug!("skipping Info"); continue },
                        MatroskaSpec::SeekHead(ebml_iterable::specs::Master::Full(_)) => { debug!("skipping SeekHead"); continue },
                        MatroskaSpec::Void(_) => { debug!("skipping Void"); continue },
                        //MatroskaSpec::Segment(Master::Start) => { debug!("skipping Segment start"); continue }, //FIX: need to fix library so that we can manually set the path to add  Segment::Start without writing, or maybe add an unchecked write
                        _ => (),
                    }
                }

                match tag {
                    MatroskaSpec::Cluster(_) => debug!("Cluster"),
                    MatroskaSpec::Segment(Master::End) => {
                        debug!("skip Segment end, will be closed with the final flush");
                        continue
                    },
                    _ => debug!("{:?}", tag),
                }

                let res = match self.internal_writer.write(&tag) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        match tag {
                            MatroskaSpec::Segment(Master::Start) => {
                                debug!("skip Segment Start");
                                continue;
                            }
                            _ => {
                                error!("tag: {:?}", tag);
                                error!("err: {:?}", e);
                            },
                        };
                        //self.internal_writer.write_unknown_size(&tag)
                        Err(e)
                    },
                };
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

    fn write_padding(&mut self, byte_count: usize) -> std::io::Result<()> {
        _ = self.internal_writer.write(&MatroskaSpec::Void(vec![0; byte_count]));
        Ok(())
    }
}





