mod util;
mod wav;

use libpulse_binding::context::{self, Context, FlagSet as ContextFlagSet};
use libpulse_binding::mainloop::threaded::Mainloop;
use libpulse_binding::proplist::Proplist;
use libpulse_binding::stream::{self, SeekMode, Stream};
use std::cell::RefCell;
use std::error::Error;
use std::ops::Deref;
use std::rc::Rc;

// NOTE: most methods of checking states and using callbacks are taken from the libpulse_binding docs
// see: https://docs.rs/libpulse-binding/2.26.0/libpulse_binding/mainloop/threaded/index.html
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // create threaded main loop
    let main_loop = Rc::new(RefCell::new(
        Mainloop::new().ok_or("Failed to create mainloop")?,
    ));

    // create pulse context
    let props = Proplist::new().ok_or("Failed to create PulseAudio Proplist")?;
    let pa_ctx = Rc::new(RefCell::new(
        Context::new_with_proplist(main_loop.borrow().deref(), "app_name", &props)
            .ok_or("Failed to create PulseAudio context")?,
    ));

    // Context state change callback
    {
        let ml_ref = Rc::clone(&main_loop);
        let context_ref = Rc::clone(&pa_ctx);
        pa_ctx
            .borrow_mut()
            .set_state_callback(Some(Box::new(move || {
                let state = unsafe { (*context_ref.as_ptr()).get_state() };
                match state {
                    context::State::Ready
                    | context::State::Failed
                    | context::State::Terminated => unsafe {
                        (*ml_ref.as_ptr()).signal(false);
                    },
                    _ => {}
                }
            })));
    }

    // connect context to pulse main loop
    pa_ctx
        .borrow_mut()
        .connect(None, ContextFlagSet::NOFLAGS, None)?;

    main_loop.borrow_mut().lock();
    main_loop.borrow_mut().start()?;

    // Wait for context to be ready
    loop {
        match pa_ctx.borrow().get_state() {
            context::State::Ready => {
                break;
            }
            context::State::Failed | context::State::Terminated => {
                main_loop.borrow_mut().unlock();
                main_loop.borrow_mut().stop();
                return Err("Context state failed/terminated, quitting...".into());
            }
            _ => {
                main_loop.borrow_mut().wait();
            }
        }
    }
    pa_ctx.borrow_mut().set_state_callback(None);

    // setup simple channel to be notified when the stream is ready --------------------------------

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    // create a pulse stream -----------------------------------------------------------------------

    {
        let (spec, audio_data) = wav::read_wav_file().await?;
        let audio_data_len = audio_data.len();

        // create pulse stream
        let stream = match Stream::new(&mut pa_ctx.borrow_mut(), "SAMPLE_NAME", &spec, None) {
            Some(stream) => Rc::new(RefCell::new(stream)),
            None => panic!("failed to create new stream"),
        };

        // Stream state change callback
        {
            let ml_ref = Rc::clone(&main_loop);
            let stream_ref = Rc::clone(&stream);
            stream
                .borrow_mut()
                .set_state_callback(Some(Box::new(move || {
                    let state = unsafe { (*stream_ref.as_ptr()).get_state() };
                    match state {
                        stream::State::Ready
                        | stream::State::Failed
                        | stream::State::Terminated => unsafe {
                            (*ml_ref.as_ptr()).signal(false);
                        },
                        _ => {}
                    }
                })));
        }

        // Stream write callback
        {
            let stream_ref = Rc::clone(&stream);
            let mut bytes_written = 0;
            stream
                .borrow_mut()
                .set_write_callback(Some(Box::new(move |len| {
                    stream_ref
                        .borrow_mut()
                        .write(&audio_data, None, 0, SeekMode::Relative)
                        .expect("failed to write to stream");

                    bytes_written += len;

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

                        tx.send(()).unwrap();
                    }
                })));
        }

        // connect the stream as an upload, which sends it to the audio server instead of playing it directly
        stream.borrow_mut().connect_upload(audio_data_len)?;

        // Wait for stream to be ready
        loop {
            // intentionally wrapping in a block to prevent multiple borrows from occurring
            // (from within the write callback)
            let stream_state = { stream.borrow().get_state() };
            match stream_state {
                stream::State::Ready => {
                    break;
                }
                stream::State::Failed | stream::State::Terminated => {
                    main_loop.borrow_mut().unlock();
                    main_loop.borrow_mut().stop();
                    return Err("Stream state failed/terminated, quitting...".into());
                }
                _ => {
                    main_loop.borrow_mut().wait();
                }
            }
        }
        stream.borrow_mut().set_state_callback(None);
    }

    main_loop.borrow_mut().unlock();

    // play the sample
    while let Some(()) = rx.recv().await {
        pa_ctx
            .borrow_mut()
            .play_sample("SAMPLE_NAME", None, None, None);
    }

    // block forever since pulse main loop is running and we don't want to stop it
    futures::future::pending::<Result<(), Box<dyn Error>>>().await
}
