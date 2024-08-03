use std::error::Error;

pub trait GuiFramework {
    fn run(app: Box<dyn AppInterface>) -> Result<(), Box<dyn Error>>;
}

pub trait AppInterface {
    fn update(&mut self);
    fn render(&self) -> String;
    fn handle_input(&mut self, input: &str);
    fn should_quit(&self) -> bool;
    fn get_query(&self) -> String;
    fn get_search_results(&self) -> Vec<String>;
    fn get_time(&self) -> String;
    fn launch_app(&mut self, app_name: &str);
}