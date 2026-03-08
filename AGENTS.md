This code is mostly ai-generated and new features should be "integration tested" by tests in the tests/ directory. There may still be some mocks injected into the integration tests but they should be injected into the complete application.

This repository contains a personal-agent like application that does things like schedule calendar events and reminder the user of things they need to do.

The main logic is in a state machine that is entered via discord prompt. After the prompt, based on inference of the prompt's intent, the state machine proceeds to take a action, sometimes prompting the user for feedback.

There are also several scheduled tasks to perform background work.

When asking the user for more information, its best to give them a simple yes/no or multiple choice decision rather than making them enter a new command.

Always run cargo test after any change and don't consider any change complete until it passes.