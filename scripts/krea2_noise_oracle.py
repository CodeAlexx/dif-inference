#!/usr/bin/env python3
"""PyTorch oracle for the Krea 2 native initial noise.
Convention: torch.randn((1,16,128,128), generator=torch.manual_seed(seed), device='cpu', dtype=float32)
-> bf16 -> rearrange 'b c (h ph) (w pw) -> b (h w) (c ph pw)' (ph=pw=2)  == initial_image_tokens [1,4096,64] bf16.
This is the ComfyUI/PyTorch CPU generator convention. The creator fixture used a CUDA Philox
generator; the compiler has no native Philox, so seeds are NOT interchangeable with the creator's."""
import sys, torch
from safetensors.torch import save_file
seed = int(sys.argv[1]); out = sys.argv[2]
g = torch.manual_seed(seed)
noise = torch.randn((1, 16, 128, 128), generator=g, device="cpu", dtype=torch.float32).to(torch.bfloat16)
b, c, H, W = noise.shape
tokens = noise.view(b, c, H // 2, 2, W // 2, 2).permute(0, 2, 4, 1, 3, 5).reshape(b, (H // 2) * (W // 2), c * 4).contiguous()
save_file({"initial_image_tokens": tokens, "noise": noise}, out)
print("oracle", out, tuple(tokens.shape), tokens.dtype, "mean %.5f std %.5f" % (tokens.float().mean(), tokens.float().std()))
