pub struct TaskRunner {
    tasks: Vec<Box<dyn FnOnce() + Send>>,
}

impl TaskRunner {
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    pub fn add_task<F>(&mut self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.tasks.push(Box::new(task));
    }

    pub fn start_all(self) {
        for task in self.tasks {
            task();
        }
    }
}
