use crate::args::TuiArgs;
use crate::tui::App;

pub(crate) fn handle_tui_command(_args: &TuiArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    app.run()?;
    Ok(())
}
