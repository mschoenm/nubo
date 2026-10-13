//! Turns Nubo's pictures (the game screen, title screens, box art and the
//! pictures of save states) into Iced image handles.
//!
//! Iced keeps an image on the graphics card for as long as its handle is
//! drawn, and knows it by the handle's id, which every new handle gets anew.
//! So each picture gets one handle, kept with the picture: drawing it again
//! uploads nothing, and the window only uploads new pictures, such as a new
//! frame of the game.

use bytes::Bytes;
use iced::widget::image;
use nubo_frontend::picture::Picture;

/// The image handle of `picture`, made the first time it is drawn and the
/// same ever after. The handle shares the picture's pixels without copying
/// them.
pub fn handle(picture: &Picture) -> image::Handle {
    picture.drawn(|picture| {
        image::Handle::from_rgba(
            picture.width(),
            picture.height(),
            Bytes::from_owner(picture.shared_pixels()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::handle;
    use nubo_frontend::picture::Picture;

    #[test]
    fn a_picture_keeps_its_handle() {
        let picture = Picture::new(1, 1, vec![1, 2, 3, 4]);

        assert_eq!(handle(&picture).id(), handle(&picture.clone()).id());
    }

    #[test]
    fn a_new_picture_gets_a_new_handle() {
        let first = Picture::new(1, 1, vec![1, 2, 3, 4]);
        let second = Picture::new(1, 1, vec![1, 2, 3, 4]);

        assert_ne!(handle(&first).id(), handle(&second).id());
    }
}
