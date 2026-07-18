---
license: mit
license_link: https://huggingface.co/microsoft/Phi-4-mini-instruct/resolve/main/LICENSE
language:
- multilingual
pipeline_tag: text-generation
tags:
- nlp
- code
- mlx
- mlx-my-repo
widget:
- messages:
  - role: user
    content: Can you provide ways to eat combinations of bananas and dragonfruits?
library_name: transformers
base_model: microsoft/Phi-4-mini-instruct
---

# pcuenq/Phi-4-mini-instruct-Q4-mlx

The Model [pcuenq/Phi-4-mini-instruct-Q4-mlx](https://huggingface.co/pcuenq/Phi-4-mini-instruct-Q4-mlx) was converted to MLX format from [microsoft/Phi-4-mini-instruct](https://huggingface.co/microsoft/Phi-4-mini-instruct) using mlx-lm version **0.21.5**.

## Use with mlx

```bash
pip install mlx-lm
```

```python
from mlx_lm import load, generate

model, tokenizer = load("pcuenq/Phi-4-mini-instruct-Q4-mlx")

prompt="hello"

if hasattr(tokenizer, "apply_chat_template") and tokenizer.chat_template is not None:
    messages = [{"role": "user", "content": prompt}]
    prompt = tokenizer.apply_chat_template(
        messages, tokenize=False, add_generation_prompt=True
    )

response = generate(model, tokenizer, prompt=prompt, verbose=True)
```
