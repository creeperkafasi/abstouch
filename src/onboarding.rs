use std::{collections::HashMap, env::args, io::Write, process::exit};

use evdev::KeyCode;

use crate::Action;

pub fn generate_config() {
    println!("--- Abstouch-WL Setup ---");
    println!();

    let executable_name = args().next().unwrap();

    let input_device = {
        println!("Supported Devices:");
        let devices = evdev::enumerate()
            // Only if the device supports absolute axes
            .filter(|(_, d)| {
                if let Ok(absinfo) = d.get_absinfo() {
                    absinfo.collect::<Vec<_>>().len() > 0
                } else {
                    false
                }
            })
            .collect::<Vec<_>>();

        for (index, (path, dev)) in devices.iter().enumerate() {
            println!(
                "{index:3}: {} ({})",
                dev.name().unwrap_or(format!("{:?}", dev).as_str()),
                path.to_string_lossy()
            )
        }
        println!();
        println!(
            "Select a device between 0 and {}: (Default: 0)",
            devices.len() - 1
        );

        let mut buffer = String::new();
        std::io::stdin()
            .read_line(&mut buffer)
            .expect("Unable to read line from stdin");

        if buffer.trim().is_empty() {
            devices[0].0.to_str().unwrap().to_string()
        } else {
            match { buffer.trim().parse::<usize>() } {
                Ok(idx) if idx < devices.len() => devices[idx].0.to_str().unwrap().to_string(),
                Ok(idx) => {
                    eprintln!(
                        "Error: Index {} is out of range. Please select a number between 0 and {}.",
                        idx,
                        devices.len() - 1
                    );
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error: Invalid input. Please enter a valid number. ({})", e);
                    std::process::exit(1);
                }
            }
        }
    };

    let action_map: HashMap<_, _> = {
        let virtual_actions = [
            (Action::Ignore, "Do nothing"),
            (Action::Scroll, "Scroll"),
            (Action::Key(KeyCode::BTN_TOUCH), "Touch"),
            (Action::Key(KeyCode::BTN_LEFT), "Left Click"),
            (Action::Key(KeyCode::BTN_RIGHT), "Right Click"),
            (Action::Key(KeyCode::BTN_TOOL_PEN), "Pen Tool"),
            (Action::Key(KeyCode::BTN_TOOL_RUBBER), "Rubber Tool"),
            (Action::Key(KeyCode::BTN_STYLUS), "Stylus"),
        ];

        let user_actions = [
            (KeyCode::BTN_TOUCH, "touch", Action::Ignore, "--on-touch"),
            (
                KeyCode::BTN_LEFT,
                "left click",
                Action::Key(KeyCode::BTN_LEFT),
                "--on-left",
            ),
            (
                KeyCode::BTN_RIGHT,
                "right click",
                Action::Key(KeyCode::BTN_LEFT),
                "--on-right",
            ),
        ];

        HashMap::from_iter(
            user_actions.map(|(_user_key, description, default, argument)| {
                println!(
                    "On {description}... (Default: {})",
                    virtual_actions
                        .iter()
                        .find(|(a, _)| *a == default)
                        .map_or(format!("{default:?}").as_str(), |(_, desc)| desc)
                );
                for (idx, (_, description)) in virtual_actions.iter().enumerate() {
                    println!("{idx:3}: {}", description);
                }
                let mut buffer = String::new();
                std::io::stdin()
                    .read_line(&mut buffer)
                    .expect("Unable to read line from stdin");

                let virtual_key = if buffer.trim().is_empty() {
                    println!("Default");
                    default
                } else {
                    virtual_actions
                        .get(buffer.trim().parse::<usize>().unwrap_or(0))
                        .expect("Please select an index in range")
                        .0
                };
                return (argument, virtual_key);
            }),
        )
    };

    let two_finger_scroll = {
        print!("Enable two finger scrolling? (Y/n) ");
        std::io::stdout().flush().unwrap();
        match {
            let mut buffer = String::new();
            std::io::stdin()
                .read_line(&mut buffer)
                .expect("Unable to read line from stdin");
            buffer.trim().to_lowercase()
        }
        .as_str()
        {
            "" | "yes" | "y" => true,
            "no" | "n" => false,
            _ => {
                eprintln!("Invalid value");
                exit(1)
            }
        }
    };

    let tap_to_click = {
        print!("Enable tap to click? (Y/n) ");
        std::io::stdout().flush().unwrap();
        match {
            let mut buffer = String::new();
            std::io::stdin()
            .read_line(&mut buffer)
            .expect("Unable to read line from stdin");
            buffer.trim().to_lowercase()
        }
        .as_str()
        {
            "" | "yes" | "y" => true,
            "no" | "n" => false,
            _ => {
                eprintln!("Invalid value");
                exit(1)
            }
        }
    };

    let double_tap_to_drag = if !tap_to_click { false } else {
        print!("Enable double tap to drag? (Y/n) ");
        std::io::stdout().flush().unwrap();
        match {
            let mut buffer = String::new();
            std::io::stdin()
            .read_line(&mut buffer)
            .expect("Unable to read line from stdin");
            buffer.trim().to_lowercase()
        }
        .as_str()
        {
            "" | "yes" | "y" => true,
            "no" | "n" => false,
            _ => {
                eprintln!("Invalid value");
                exit(1)
            }
        }
    };

    let scroll_factor = { 1.0 };

    println!("Command to run your config is:");
    println!(
        "{executable_name} -i {input_device} {} -s {scroll_factor}{}{}{}",
        action_map
            .iter()
            .map(|(arg, action)| {
                format!(
                    "{arg} {}",
                    match action {
                        Action::Key(action) => action.code().to_string(),
                        Action::Scroll => "scroll".to_string(),
                        Action::Ignore => "ignore".to_string(),
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(" "),
        if two_finger_scroll { "" } else { " --disable-mt-scroll" },
        if tap_to_click { "" } else { " --disable-ttc" },
        if double_tap_to_drag { "" } else { " --disable-dttd" }
    );
}
