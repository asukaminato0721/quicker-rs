use super::*;

impl QuickerRuntime {
    fn integer_input(
        &self,
        params: &Map<String, Value>,
        key: &str,
        default: i64,
    ) -> Result<i64, String> {
        let Some(value) = self.input_value(params, key)? else {
            return Ok(default);
        };
        if value == Value::String(String::new()) {
            return Ok(default);
        }
        if let Value::Number(number) = &value {
            if let Some(integer) = number.as_i64() {
                return Ok(integer);
            }
            if let Some(float) = number.as_f64() {
                if float.fract() == 0.0 && float >= i64::MIN as f64 && float < -(i64::MIN as f64) {
                    return Ok(float as i64);
                }
            }
            return Err(format!("Input {key} requires an integer within range"));
        }
        let value = expression::convert(value, Some(12))?;
        value
            .as_i64()
            .ok_or_else(|| format!("Input {key} requires an integer"))
    }

    pub(super) fn run_repeat(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let count = self.integer_input(&step.input_params, "count", 1)?;
        let start = self.integer_input(&step.input_params, "startIndex", 0)?;
        let delay = self.integer_input(&step.input_params, "repeatDelayMs", 1)?;
        if delay < 0 {
            return Err("Input repeatDelayMs cannot be negative".into());
        }
        let mut iteration = 0_i64;
        while count == -1 || iteration < count {
            ensure_not_cancelled(self.control.as_ref())?;
            let index = start
                .checked_add(iteration)
                .ok_or("Repeat index overflow")?;
            // Quicker 1.45.5 writes the index before it evaluates stopCondition.
            self.assign_output(&step.output_params, "count", Value::from(index))?;
            if self.input_bool(&step.input_params, "stopCondition")? {
                break;
            }
            match self.run_steps(step.if_steps.as_deref().unwrap_or(&[]))? {
                StepFlow::Continue | StepFlow::NextIteration => {}
                StepFlow::BreakLoop => break,
                stop @ (StepFlow::Stop(_) | StepFlow::StopAction(_)) => return Ok(stop),
            }
            iteration = iteration.checked_add(1).ok_or("Repeat counter overflow")?;
            if count == -1 || iteration < count {
                sleep_millis(delay as u64, self.control.as_ref())?;
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::yield_now();
            }
        }
        Ok(StepFlow::Continue)
    }

    pub(super) fn run_each(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        // Do not hide an unsupported execution mode behind stopIfFail=false.
        if self.input_bool(&step.input_params, "useMultiThread")? {
            return Err("Parallel each execution is not supported".into());
        }
        let stop_on_failure = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|value| truthy(Some(&value)));
        let result = self.run_each_sequential(step);
        // Cancellation must propagate even when stopIfFail is false.
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "isSuccess",
            Value::Bool(result.is_ok()),
        )?;
        match result {
            Err(_) if !stop_on_failure => Ok(StepFlow::Continue),
            other => other,
        }
    }

    fn run_each_sequential(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let input = self
            .input_value(&step.input_params, "input")?
            .ok_or("Missing input param: input")?;
        let input = expression::convert(input, Some(4))?;
        let items = input.as_array().ok_or("Input input requires a list")?;
        for (index, item) in items.iter().enumerate() {
            ensure_not_cancelled(self.control.as_ref())?;
            self.assign_output(&step.output_params, "item", item.clone())?;
            self.assign_output(&step.output_params, "count", Value::from(index))?;
            match self.run_steps(step.if_steps.as_deref().unwrap_or(&[]))? {
                StepFlow::Continue | StepFlow::NextIteration => {}
                StepFlow::BreakLoop => break,
                stop @ (StepFlow::Stop(_) | StepFlow::StopAction(_)) => return Ok(stop),
            }
        }
        Ok(StepFlow::Continue)
    }
}
