mod onboarding;

use std::{collections::HashMap, env::args, time::{Duration, Instant}};

use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode,
    UinputAbsSetup, uinput::VirtualDevice,
};

#[derive(Debug, PartialEq, Clone, Copy)]
enum Action {
    Ignore,
    Key(KeyCode),
    Scroll,
}

fn main() {
    // evdev won't work without root
    if !nix::unistd::Uid::effective().is_root() {
        println!("You must run this program as root!");
        return;
    }

    let mut input_device_path = None;
    let mut input_map = HashMap::new();
    let mut scroll_factor = 1.0; // Adjust scroll sensitivity
    let mut mt_scroll_enabled = true;
    let mut ttc_enabled = true;
    let mut dttd_enabled = true;

    let mut args = args().peekable();
    args.next(); // Skip executable name
    if args.peek().is_none() {
        onboarding::generate_config();
        return;
    }
    fn parse_key_arg(arg: String) -> Action {
        match arg.as_str() {
            "ignore" => Action::Ignore,
            "scroll" => Action::Scroll,
            keycode => Action::Key(KeyCode::new(keycode.parse().expect("Expected an integer"))),
        }
    }

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" | "-i" => input_device_path = Some(args.next().expect("Expected argument")),
            "--on-touch" => {
                input_map.insert(
                    KeyCode::BTN_TOUCH,
                    parse_key_arg(args.next().expect("Expected argument")),
                );
            }
            "--on-left" => {
                input_map.insert(
                    KeyCode::BTN_LEFT,
                    parse_key_arg(args.next().expect("Expected argument")),
                );
            }
            "--on-right" => {
                input_map.insert(
                    KeyCode::BTN_RIGHT,
                    parse_key_arg(args.next().expect("Expected argument")),
                );
            }
            "-s" | "--scroll-factor" => {
                scroll_factor = args
                    .next()
                    .expect("Expected argument")
                    .parse()
                    .expect("Expected a float")
            }
            "--disable-mt-scroll" => mt_scroll_enabled = false,
            "--disable-ttc" => ttc_enabled = false,
            "--disable-dttd" => dttd_enabled = false,
            "--on-tap" => {
                input_map.insert(
                    KeyCode::BTN_LEFT,
                    parse_key_arg(args.next().expect("Expected argument"))
                );
            }
            _ => {
                panic!("Unknown argument: {arg}")
            }
        }
    }
    let input_device_path = input_device_path.expect("--input is required!");

    let mut touchpad = evdev::Device::open(&input_device_path)
        .expect(&format!("Unable to open device \"{}\"", input_device_path));

    let (x_abs, y_abs) = {
        let mut absinfo = touchpad.get_absinfo().unwrap();
        (
            absinfo
                .find(|(code, _)| *code == AbsoluteAxisCode::ABS_X)
                .expect("Device doesn't support ABS_X")
                .1,
            absinfo
                .find(|(code, _)| *code == AbsoluteAxisCode::ABS_Y)
                .expect("Device doesn't support ABS_Y")
                .1,
        )
    };

    touchpad.set_nonblocking(true).unwrap();

    println!(
        "[INFO] Geometry -> X: {}-{}, Y: {}-{}",
        x_abs.minimum(),
             x_abs.maximum(),
        y_abs.minimum(),
        y_abs.maximum()
    );

    /*  CREATE VIRTUAL TABLET  */
    let mut keys = AttributeSet::<KeyCode>::new();
    // Always include BTN_TOOL_PEN so we are detected as a tablet
    keys.insert(KeyCode::BTN_TOOL_PEN);
    for (_, action) in &input_map {
        if let Action::Key(key) = action {
            keys.insert(*key);
        }
    }

    let mut virtual_tablet = VirtualDevice::builder()
        .unwrap()
        .name("Abstouch-WL Tablet")
        .with_keys(&keys)
        .unwrap()
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_X,
            AbsInfo::new(0, x_abs.minimum(), x_abs.maximum(), 0, 0, 100),
        ))
        .unwrap()
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_Y,
            AbsInfo::new(0, y_abs.minimum(), y_abs.maximum(), 0, 0, 100),
        ))
        .unwrap()
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_PRESSURE,
            AbsInfo::new(0, 0, 2048, 0, 0, 0),
        ))
        .unwrap()
        .build()
        .unwrap();

    /*  CREATE VIRTUAL MOUSE FOR SCROLLING  */
    let mut rel_axes = AttributeSet::<RelativeAxisCode>::new();
    rel_axes.insert(RelativeAxisCode::REL_WHEEL);
    rel_axes.insert(RelativeAxisCode::REL_HWHEEL);
    rel_axes.insert(RelativeAxisCode::REL_WHEEL_HI_RES);
    rel_axes.insert(RelativeAxisCode::REL_HWHEEL_HI_RES);

    let mut virtual_mouse = VirtualDevice::builder()
        .unwrap()
        .name("Abstouch-WL Scroll Device")
        .with_relative_axes(&rel_axes)
        .unwrap()
        .build()
        .unwrap();

    println!("\n[SUCCESS] Virtual Devices Created!");

    touchpad.grab().unwrap();
    println!("[INFO] Touchpad grabbed. Press Ctrl+C to stop.");

    // Track multitouch state
    let mut active_slot: Option<i32> = None; // The slot we're tracking (first finger)
    let mut current_slot: i32 = 0; // Current slot being reported
    let mut finger_count: i32 = 0; // Number of fingers currently down

    // For two-finger scroll
    let mut last_y: Option<i32> = None;
    let mut last_x: Option<i32> = None;
    // let mut old_now = chrono::Local::now();

    let mut press_start = Instant::now();

    let mut lift_start = Instant::now();

    let mut double_tap: bool = false;
    let mut dragging: bool = false;

    let mut x_position = None;
    let mut y_position = None;

    loop {
        //dbg!(finger_count);
        //let events = touchpad.fetch_events().unwrap().collect::<Vec<_>>();

        if let Ok(fetched) = touchpad.fetch_events() {
            let events = fetched.collect::<Vec<_>>();

            // println!("Batch size: {:?}", events.len());

            for event in events {
                match event.event_type() {
                    EventType::ABSOLUTE => {
                        let code = AbsoluteAxisCode(event.code());
                        match code {
                            AbsoluteAxisCode::ABS_MT_SLOT => {
                                current_slot = event.value();
                            }
                            AbsoluteAxisCode::ABS_MT_TRACKING_ID => {
                                match event.value() {
                                    -1 => {
                                        // Finger lifted
                                        finger_count -= 1;
                                        if active_slot == Some(current_slot) {
                                            active_slot = None;

                                            //println!("press duration : {:?}", press_start.elapsed());

                                            if ttc_enabled {

                                                lift_start = Instant::now();

                                                if !dragging {

                                                    if press_start.elapsed() < Duration::from_millis(200) {
                                                        println!("click !");

                                                        virtual_tablet
                                                        .emit(&[InputEvent::new(
                                                            EventType::ABSOLUTE.0,
                                                            AbsoluteAxisCode::ABS_PRESSURE.0,
                                                            2048,
                                                        )])
                                                        .unwrap();

                                                        double_tap = true;
                                                    }

                                                } else {
                                                    virtual_tablet
                                                    .emit(&[InputEvent::new(
                                                        EventType::ABSOLUTE.0,
                                                        AbsoluteAxisCode::ABS_PRESSURE.0,
                                                        0,
                                                    )])
                                                    .unwrap();
                                                    dragging = false;
                                                }
                                            }

                                        }
                                        if finger_count <= 0 {
                                            if finger_count < 0 {
                                                dbg!(finger_count);
                                            }
                                            finger_count = 0;
                                            last_x = None;
                                            last_y = None;
                                        }
                                    }
                                    _ => {
                                        // New finger

                                        if ttc_enabled && finger_count == 0 {
                                            if double_tap {
                                                dragging = true;
                                            } else {
                                                double_tap = false;
                                                press_start = Instant::now();
                                            }
                                        }

                                        finger_count += 1;
                                        if active_slot.is_none() {
                                            active_slot = Some(current_slot);
                                        }
                                    }
                                }
                            }
                            // Single finger - move tablet
                            AbsoluteAxisCode::ABS_X | AbsoluteAxisCode::ABS_MT_POSITION_X
                                if active_slot == Some(current_slot)
                                    && (finger_count < 2 || !mt_scroll_enabled) =>
                            {
                                if event.value() != 0 {
                                    x_position = Some(event.value());
                                }
                            }
                            // Y axis movement
                            AbsoluteAxisCode::ABS_Y | AbsoluteAxisCode::ABS_MT_POSITION_Y
                                if active_slot == Some(current_slot)
                                    && (finger_count < 2 || !mt_scroll_enabled) =>
                            {
                                y_position = Some(event.value());
                            }
                            // Two-finger scroll
                            AbsoluteAxisCode::ABS_MT_POSITION_X
                            | AbsoluteAxisCode::ABS_MT_POSITION_Y
                                if active_slot == Some(current_slot)
                                    && finger_count >= 2
                                    && mt_scroll_enabled =>
                            {
                                let (last, rel_code, mult) = match code {
                                    AbsoluteAxisCode::ABS_MT_POSITION_X => {
                                        (last_x, RelativeAxisCode::REL_HWHEEL_HI_RES, 1)
                                    }
                                    AbsoluteAxisCode::ABS_MT_POSITION_Y => {
                                        (last_y, RelativeAxisCode::REL_WHEEL_HI_RES, -1)
                                    }
                                    _ => unreachable!(),
                                };
                                if let Some(l) = last {
                                    let delta = ((event.value() - l) as f32 * scroll_factor) as i32;
                                    if delta != 0 {
                                        virtual_mouse
                                            .emit(&[InputEvent::new(
                                                EventType::RELATIVE.0,
                                                rel_code.0,
                                                delta * mult,
                                            )])
                                            .unwrap();
                                    }
                                }
                                match code {
                                    AbsoluteAxisCode::ABS_MT_POSITION_X => {
                                        last_x = Some(event.value());
                                    }
                                    AbsoluteAxisCode::ABS_MT_POSITION_Y => {
                                        last_y = Some(event.value());
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                    EventType::KEY => {
                        let code = KeyCode(event.code());
                        if let Some(action) = input_map.get(&code) {
                            match action {
                                Action::Key(action) => {
                                    virtual_tablet
                                        .emit(&[InputEvent::new(
                                            EventType::KEY.0,
                                            action.code(),
                                            event.value(),
                                        )])
                                        .unwrap();
                                    let pressure = if event.value() == 1 { 2048 } else { 0 };
                                    virtual_tablet
                                        .emit(&[InputEvent::new(
                                            EventType::ABSOLUTE.0,
                                            AbsoluteAxisCode::ABS_PRESSURE.0,
                                            pressure,
                                        )])
                                        .unwrap();
                                }
                                Action::Scroll => todo!("Scrolling with a button"),
                                Action::Ignore => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        //dbg!(x_position, y_position);

        if let (Some(x_position_value), Some(y_position_value)) = (x_position, y_position) {
            //println!("x: {x_position_value}, y: {y_position_value}");
            virtual_tablet
                .emit(&[InputEvent::new(
                    EventType::ABSOLUTE.0,
                    AbsoluteAxisCode::ABS_X.0,
                    x_position_value,
                )])
                .unwrap();

            virtual_tablet
                .emit(&[InputEvent::new(
                    EventType::ABSOLUTE.0,
                    AbsoluteAxisCode::ABS_Y.0,
                    y_position_value,
                )])
                .unwrap();
            //x_position = None;
            //y_position = None;
        }

        if dttd_enabled
            && double_tap
            && !dragging
            && lift_start.elapsed() > Duration::from_millis(200)
        {
            println!("stop click!");
            virtual_tablet
            .emit(&[InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_PRESSURE.0,
                0
            )])
            .unwrap();
            double_tap = false;
        }

        // Synchronize after each event batch
        virtual_tablet
            .emit(&[InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0)])
            .unwrap();
        virtual_mouse
            .emit(&[InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0)])
            .unwrap();
        // let now = chrono::Local::now();
        // let delta_time = now.signed_duration_since(old_now);
        // old_now = now;
        // dbg!(delta_time);
    }
}
