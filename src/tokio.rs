mod util;
mod wav;

use std::{cell::RefCell, error::Error, rc::Rc};

use libpulse_binding::{
    context::{Context, FlagSet, State},
    proplist::Proplist,
    stream::{SeekMode, Stream},
};
use libpulse_tokio::TokioMain;
use tokio::{runtime::Builder, task::LocalSet};

fn main() {
    // single threaded runtime
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    // wrapped in a tokio local set (to enable `tokio::task::spawn_local`)
    let output = runtime.block_on(async { LocalSet::new().run_until(async_main()).await });
    output.expect("an error occurred");
}

async fn async_main() -> Result<(), Box<dyn Error>> {
    // setup pulse main loop api -------------------------------------------------------------------

    let (mut main_loop, mut pa_ctx) = {
        // create tokio mainloop
        let mut main_loop = TokioMain::new();

        // create pulse context
        let props = Proplist::new().ok_or("Failed to create PulseAudio Proplist")?;
        let mut pa_ctx = Context::new_with_proplist(&main_loop, "app_name", &props)
            .ok_or("Failed to create PulseAudio context")?;

        // connect it and wait for mainloop to be ready
        pa_ctx.connect(None, FlagSet::NOFAIL, None).unwrap();
        if !matches!(main_loop.wait_for_ready(&pa_ctx).await, Ok(State::Ready)) {
            panic!("mainloop error");
        }

        (main_loop, pa_ctx)
    };

    // spawn task to run pulse main loop
    tokio::task::spawn_local(async move {
        let ret = main_loop.run().await;
        eprintln!("exited with return value: {}", ret.0);
    });

    // setup simple channel to be notified when the stream is ready --------------------------------

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    // create a pulse stream -----------------------------------------------------------------------

    let (spec, audio_data) = wav::read_wav_file().await?;
    let audio_data_len = audio_data.len();

    // create pulse stream
    let stream = match Stream::new(&mut pa_ctx, "SAMPLE_NAME", &spec, None) {
        Some(stream) => Rc::new(RefCell::new(stream)),
        None => panic!("failed to create new stream"),
    };

    // set up write callback for writing audio data to the stream
    let stream_ref = stream.clone();
    let mut bytes_written = 0;
    stream
        .borrow_mut()
        .set_write_callback(Some(Box::new(move |len| {
            // write audio data to stream
            stream_ref
                .borrow_mut()
                .write(&audio_data, None, 0, SeekMode::Relative)
                .expect("failed to write to stream");

            bytes_written += len;

            // we're finished writing the audio data, finish the upload, thereby saving the audio stream
            // as a sample in the audio server (so we can play it later)
            if bytes_written == audio_data.len() {
                // FIXME !!!! FIXME !!!!
                // a segmentation fault occurs when calling `.set_write_callback` here
                if util::should_unset_write_callback() {
                    stream_ref.borrow_mut().set_write_callback(None);
                }

                // we're done writing the audio data, tell the server to convert this stream to a sample
                stream_ref
                    .borrow_mut()
                    .finish_upload()
                    .expect("failed to finish upload");

                // stream is ready
                tx.send(()).unwrap();
            }
        })));

    // connect the stream as an upload, which sends it to the audio server instead of playing it directly
    stream.borrow_mut().connect_upload(audio_data_len)?;

    // play the sample
    while let Some(()) = rx.recv().await {
        pa_ctx.play_sample("SAMPLE_NAME", None, None, None);
    }

    // block forever since pulse main loop is running and we don't want to stop it
    futures::future::pending::<Result<(), Box<dyn Error>>>().await
}
