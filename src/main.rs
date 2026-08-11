fn main() {
    #[cfg(target_env = "ohos")]
    {
        use macroquad::miniquad::native;
        native::set_interceptor_state(true);
    }
    audio_tester_rs::quad_main();
}
