use std::error::Error;

use libpulse_binding::sample::{Format, Spec};

use tokio::{
    fs::{File, OpenOptions},
    io::AsyncReadExt,
};

pub async fn read_wav_file() -> Result<(Spec, Vec<u8>), Box<dyn Error>> {
    // open file
    let file = OpenOptions::new().read(true).open("pop.wav").await?;

    // get its metadata
    let meta = file.metadata().await?;

    // now use `hound` to read the wav specification
    let wav_reader = hound::WavReader::new(file.into_std().await)?;
    let wav_spec = wav_reader.spec();

    // convert back to an async `File` to read the rest of the data now that the `WavReader` has
    // read the header and metadata parts
    let mut file = File::from_std(wav_reader.into_inner());

    // read the rest of the file (the audio data)
    let mut buf = Vec::with_capacity(meta.len() as usize);
    file.read_to_end(&mut buf).await?;

    // create a pulse spec from the wav spec
    let spec = Spec {
        format: match wav_spec.sample_format {
            hound::SampleFormat::Float => Format::FLOAT32NE,
            hound::SampleFormat::Int => match wav_spec.bits_per_sample {
                16 => Format::S16NE,
                24 => Format::S24NE,
                32 => Format::S32NE,
                n => panic!("unsupported bits per sample: {}", n),
            },
        },
        channels: wav_spec.channels as u8,
        rate: wav_spec.sample_rate,
    };

    if !spec.is_valid() {
        panic!("format specification wasn't valid: {:?}", spec);
    }

    // pad out sound data to the next frame size
    let frame_size = spec.frame_size();
    if let Some(rem) = buf.len().checked_rem(frame_size) {
        buf.extend(vec![0; rem]);
    }

    Ok((spec, buf))
}
