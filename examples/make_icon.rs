// cargo run --example make_icon  (assets/icon.svg -> assets/icon.png, assets/icon.ico)
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::{ExtendedColorType, RgbaImage};
use resvg::{tiny_skia, usvg};

fn render(tree: &usvg::Tree, size: u32) -> RgbaImage {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("pixmap");
    let scale = size as f32 / tree.size().width();
    resvg::render(tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let mut img = RgbaImage::new(size, size);
    for (dst, src) in img.pixels_mut().zip(pixmap.pixels()) {
        let c = src.demultiply();
        *dst = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    img
}

fn main() {
    let svg = std::fs::read("assets/icon.svg").expect("read assets/icon.svg");
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).expect("parse svg");

    render(&tree, 256).save("assets/icon.png").expect("write icon.png");

    let frames: Vec<IcoFrame> = [16u32, 24, 32, 48, 64, 128, 256]
        .iter()
        .map(|&s| {
            let img = render(&tree, s);
            IcoFrame::as_png(img.as_raw(), s, s, ExtendedColorType::Rgba8).expect("frame")
        })
        .collect();
    let file = std::fs::File::create("assets/icon.ico").expect("create icon.ico");
    IcoEncoder::new(file).encode_images(&frames).expect("write icon.ico");
    println!("wrote assets/icon.png and assets/icon.ico");
}
