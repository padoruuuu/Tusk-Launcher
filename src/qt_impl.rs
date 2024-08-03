use qt_widgets::qt_core::{q_init_resource, QBox, QObject, QTimer, SlotNoArgs};
use qt_widgets::qt_gui::QIcon;
use qt_widgets::qt_widgets::{QApplication, QLineEdit, QListWidget, QMainWindow, QPushButton, QVBoxLayout, QWidget};
use crate::gui_trait::{GuiFramework, AppInterface};

pub struct QtGui;

impl GuiFramework for QtGui {
    fn run(mut app: Box<dyn AppInterface>) -> Result<(), Box<dyn std::error::Error>> {
        QApplication::init(|_| {
            let mut window = QMainWindow::new_0a();
            window.set_window_title(&QString::from_std_str("Application Launcher"));
            window.resize_2a(300, 400);

            let central_widget = QWidget::new_0a();
            window.set_central_widget(central_widget.into_ptr());

            let layout = QVBoxLayout::new_1a(central_widget.widget());

            let search_bar = QLineEdit::new();
            layout.add_widget(search_bar.into_ptr());

            let results_list = QListWidget::new_0a();
            layout.add_widget(results_list.into_ptr());

            let time_label = QLabel::new();
            layout.add_widget(time_label.into_ptr());

            let power_button = QPushButton::from_q_string(&QString::from_std_str("Power"));
            let restart_button = QPushButton::from_q_string(&QString::from_std_str("Restart"));
            let logout_button = QPushButton::from_q_string(&QString::from_std_str("Logout"));

            let button_layout = QHBoxLayout::new_0a();
            button_layout.add_widget(power_button.into_ptr());
            button_layout.add_widget(restart_button.into_ptr());
            button_layout.add_widget(logout_button.into_ptr());

            layout.add_layout_1a(button_layout.into_ptr());

            let update_ui = SlotNoArgs::new(move || {
                search_bar.set_text(&QString::from_std_str(&app.get_query()));
                
                results_list.clear();
                for result in app.get_search_results() {
                    results_list.add_item_q_string(&QString::from_std_str(&result));
                }

                time_label.set_text(&QString::from_std_str(&app.get_time()));
            });

            search_bar.text_edited().connect(&SlotNoArgs::new(move || {
                app.handle_input(&search_bar.text().to_std_string());
                update_ui.emit();
            }));

            results_list.item_double_clicked().connect(&SlotNoArgs::new(move || {
                if let Some(item) = results_list.current_item() {
                    app.launch_app(&item.text().to_std_string());
                }
            }));

            power_button.clicked().connect(&SlotNoArgs::new(move || {
                app.handle_input("P");
            }));

            restart_button.clicked().connect(&SlotNoArgs::new(move || {
                app.handle_input("R");
            }));

            logout_button.clicked().connect(&SlotNoArgs::new(move || {
                app.handle_input("L");
            }));

            let timer = QTimer::new_1a(&window);
            timer.timeout().connect(&update_ui);
            timer.start_1a(1000);

            window.show();
            QApplication::exec()
        });

        Ok(())
    }
}