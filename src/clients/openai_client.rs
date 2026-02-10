use serde::{Deserialize, Serialize};
use chrono::Utc;
use chrono::DateTime;
use reqwest;

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: String,
}


pub async fn generate_openai_prompt(
    prompt: &str,
    prompt_type: &str,
    api_key: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let now: DateTime<Utc> = Utc::now();

    let full_prompt = match prompt_type {
        "notification" => format!(
            "You are a notification extraction engine.\n\
             Current date and time (UTC): {now}\n\
             User timezone: America/New_York\n\
             Task: From the user message below, extract:\n\
             - \"content\": the core notification text with extraneous scheduling words removed. For example:\n\
               - \"buy eggs tomorrow\" -> \"buy eggs\"\n\
               - \"notify me to call mom at 5\" -> \"call mom\"\n\
             - \"time\": an RFC3339 datetime string in the user's timezone.\n\
             Rules:\n\
             - If the user gives an explicit date like \"December 6th\", use that exact month and day at noon in the local timezone; do NOT change them.\n\
             - If the year is omitted, assume the next occurrence of that date on or after the current date.\n\
             - If the user gives a relative time (e.g. \"in two weeks\", \"tomorrow at 3pm\"), compute the concrete datetime from the current date/time.\n\
             - For day-of-week phrases:\n\
               - \"Saturday\" or \"this Saturday\" means the next occurrence of that weekday on or after today.\n\
               - \"next Saturday\" means the occurrence in the following week (at least 7 days after today), not the immediate upcoming one.\n\
             - If the time expression is unclear or missing (e.g. \"soon\", \"later\"), set the time to exactly 24 hours after the current datetime.\n\
             - If the user includes corrections or clarifications (e.g. \"actually I meant this Saturday\"), treat them as time corrections only and DO NOT include them in \"content\".\n\
             - If the message contains a \"Context notes\" or \"Additional context\" section, never copy that text into \"content\".\n\
             - Never invent or adjust the date away from what the user wrote; only add a year or time if needed.\n\
             - Output ONLY raw JSON, no prose, markdown, or code fences.\n\
             - The JSON shape must be exactly:\n\
             {{\"content\":\"<string>\",\"time\":\"<RFC3339 datetime>\"}}\n\
             User message: \"{user_prompt}\"",
            now = now.to_rfc3339(),
            user_prompt = prompt
        ),
        "notification_correction" => format!(
            "You are a notification correction engine.\n\
             Current date and time (UTC): {now}\n\
             User timezone: America/New_York\n\
             Task: Given the original notification request and a user-provided correction note, output a corrected notification.\n\
             Rules:\n\
             - The correction note is NOT notification content. It is only for fixing the date/time or clarifying intent.\n\
             - Preserve the original notification content unless the correction explicitly changes it.\n\
             - If the correction only adjusts time (e.g. \"actually I meant this Saturday\"), update only the time.\n\
             - Output ONLY raw JSON, no prose, markdown, or code fences.\n\
             - The JSON shape must be exactly:\n\
             {{\"content\":\"<string>\",\"time\":\"<RFC3339 datetime>\"}}\n\
             Original request: \"{user_prompt}\"",
            now = now.to_rfc3339(),
            user_prompt = prompt
        ),
        "notification_message" => format!(
            "You are a notification message formatter.\n\
             Current date and time (UTC): {now}\n\
             Task: Given the structured notification info below, write a short, natural English notification message to send to a user.\n\
             Rules:\n\
             - Address the user(s) in second person (\"you\").\n\
             - Mention the event time explicitly.\n\
             - Include the notification content naturally.\n\
             - If hours remaining is provided, include it in a friendly way.\n\
             - Keep it to 1–2 sentences, no markdown, no lists, no JSON.\n\
             - Do NOT wrap the output in quotes.\n\
             Structured input:\n\
             {structured}",
            now = now.to_rfc3339(),
            structured = prompt
        ),
        "intent_router" => format!(
            "You are an intent router for a notification bot.\n\
             Current date and time (UTC): {now}\n\
             User timezone: America/New_York\n\
             Task: Classify the user's message into one of these intents:\n\
             - notification: requests that include a time/date for a notification\n\
             - todolist: requests to create or update a todo list without a time\n\
             - unknown: unclear or missing time/action\n\
             Rules:\n\
             - If the message contains any explicit or implicit time/date (e.g., \"tomorrow\", \"next week\", weekdays, months, \"at 5pm\"), choose notification.\n\
             - If the message contains do, or finish, or check or similar words, its a todolist. \n\
             Output ONLY raw JSON, no prose, markdown, or code fences.\n\
             The JSON shape must be exactly:\n\
             {{\"intent\":\"notification|todolist|unknown\",\"normalized_text\":\"<cleaned user text>\"}}\n\
             User message: \"{user_prompt}\"",
            now = now.to_rfc3339(),
            user_prompt = prompt
        ),
        _ => return Err("Not a valid base prompt".to_string().into()),
    };

    query_openai(full_prompt, prompt_type, api_key).await
}

async fn query_openai(
    prompt: String,
    prompt_type: &str,
    api_key: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let system_message = match prompt_type {
        "notification" | "notification_correction" => {
            "You are a strict JSON notification extraction engine. You read instructions and a user message and reply ONLY with a single JSON object, with no markdown, no backticks, and no extra text. If the user gives an explicit date (e.g. \"December 6th\"), you preserve that exact month and day and only fill in missing year/time according to the instructions."
        }
        "intent_router" => {
            "You are a strict JSON intent router. Reply ONLY with a single JSON object, with no markdown, no backticks, and no extra text."
        }
        "notification_message" => {
            "You are a notification message formatter. Reply with plain text only (no JSON, no markdown, no quotes)."
        }
        _ => "You are a helpful assistant.",
    };

    let request: OpenAIRequest = OpenAIRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system_message.to_string(),
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ],
        max_tokens: 1500,
        temperature: 0.2,
    };

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await?;

        let status = response.status();
        let text = response.text().await?; // read the body once
        
        if !status.is_success() {
            // Non-2xx response — show raw body for debugging
            println!("Error {}: {}", status, text);
            return Err(format!("Request failed with status {}", status).into());
        }
        
        // Try to parse JSON
        let parsed: OpenAIResponse = serde_json::from_str(&text).map_err(|e| {
            format!(
                "Failed to parse JSON: {}\nRaw body: {}",
                e, text
            )
        })?;
        
        // Extract the choice content
        if let Some(choice) = parsed.choices.first() {
            Ok(choice.message.content.clone())
        } else {
            println!("No choices found in response.\nRaw body:\n{}", text);
            Err("No response from OpenAI".to_string().into())
        }
}
