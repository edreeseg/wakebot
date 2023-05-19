use crate::errors::WakeBotError;
use rand::Rng;
use regex::Regex;
use shunting::{MathContext, ShuntingParser};

pub const INDIVIDUAL_ROLL_REGEX: &str = r"(\d+)?d(\d+)((k|(kh)|(kl))(\d+))?";
pub const ROLL_REGEX: &str = r"^!(((\d+)?d(\d+)((k|(kh)|(kl))(\d+))?)| |\d+|[+*/)()-])+$";
const MAX_QUANTITY: usize = 1000;

// Need to create human-readable summary of rolls

fn resolve_dice_roll(roll_string: &str) -> Result<(String, Vec<i32>, Vec<i32>), WakeBotError> {
    // let roll_string = roll_string.trim_matches(|c: char| c.eq(&'(') || c.eq(&')'));
    let roll_regex = Regex::new(INDIVIDUAL_ROLL_REGEX).unwrap();
    if !roll_regex.is_match(roll_string) {
        return Err(WakeBotError::new(
            "Invalid argument passed to resolve_dice_roll.",
        ));
    }
    let capture = roll_regex
        .captures_iter(roll_string)
        .next()
        .ok_or(WakeBotError::new(
            "Problem while accessing captured values for resolve_dice_roll.",
        ))?;
    // println!("{:?}", &capture);
    // println!("{}", &capture[0]);
    let quantity = if roll_string.chars().nth(0).unwrap().is_numeric() {
        (&capture[1]).parse::<usize>().unwrap()
    } else {
        1
    };
    if quantity > MAX_QUANTITY {
        panic!("Max number of dice is {}", MAX_QUANTITY);
    }
    let max = (&capture[2]).parse::<i32>().unwrap();
    let mut total = 0;
    let mut rolls = vec![];
    let mut discarded_rolls = vec![];
    for _ in 0..quantity {
        let roll_result: i32 = rand::thread_rng().gen_range(1..=max);
        rolls.push(roll_result);
    }
    println!("{:?}", capture);
    let advantage_type = if let Some(_) = capture.get(4) {
        Some(&capture[4])
    } else {
        None
    };
    if let Some(t) = advantage_type {
        rolls.sort();
        let count = (&capture[7]).parse::<usize>().unwrap();
        if count < quantity {
            if t.eq("k") || t.eq("kh") {
                rolls.reverse();
            }
            println!("{:?}", rolls);
            discarded_rolls = rolls.splice(count.., vec![]).collect::<Vec<i32>>();
            println!("{:?}", rolls);
        }
    }
    println!(
        "Rolled {}, {} = {}",
        roll_string,
        rolls
            .iter()
            .map(|n| {
                total += n;
                n.to_string()
            })
            .collect::<Vec<String>>()
            .join(" + "),
        total
    );
    Ok((total.to_string(), rolls, discarded_rolls))
}

pub fn calculate_roll_string(roll: &str) -> (f64, Vec<(String, String, Vec<i32>, Vec<i32>)>) {
    let regex = Regex::new(INDIVIDUAL_ROLL_REGEX).unwrap();
    let mut roll = String::from(roll);

    let mut done = false;
    let mut roll_representation: Vec<(String, String, Vec<i32>, Vec<i32>)> = vec![];
    while !done {
        let mut resolved_roll = None;
        let range = regex.find(&roll).map(|mat| {
            if let Ok((result, rolls, discarded_rolls)) = resolve_dice_roll(mat.as_str()) {
                resolved_roll = Some(result);
                roll_representation.push((
                    resolved_roll.clone().unwrap(),
                    String::from(mat.as_str()),
                    rolls,
                    discarded_rolls,
                ))
            } else {
                panic!("Matched w/ range but no dice resolution.");
            }
            mat.start()..mat.end()
        });
        if let Some(r) = range {
            roll.replace_range(r, &resolved_roll.unwrap());
        } else {
            done = true;
        }
    }
    let expr = ShuntingParser::parse_str(&roll[1..]).unwrap();
    let result = MathContext::new().eval(&expr).unwrap();
    (result, roll_representation)
}
