"""Context Manager for LLM Providers.

Handles token counting, message pruning, and context window management 
to prevent API crashes (Context Length Exceeded).
"""
from __future__ import annotations

import logging
from typing import List, Dict, Any, Optional
import math
import re

# Try to import tiktoken, fallback to heuristic
try:
    import tiktoken
    TIKTOKEN_AVAILABLE = True
except ImportError:
    TIKTOKEN_AVAILABLE = False

from app.agent.providers.base import Message, Role

logger = logging.getLogger(__name__)

class ContextManager:
    """
    Smart context manager for LLM interactions.
    
    Strategy: 'Rolling Window with System Priority'
    1. Always keep System Prompt (Project Rules).
    2. Always keep last N messages (Conversation coherence).
    3. Truncate massive Tool Outputs (Files > 20k chars).
    4. Drop oldest non-system messages until under limit.
    """
    
    def __init__(self, model: str = "gpt-4o", max_tokens: int = 128000):
        self.model = model
        self.max_tokens = max_tokens
        self.output_reserve = 4096 # Reserve tokens for model output
        self.safety_margin = 1000  # Extra safety buffer
        
        # Encoding setup
        self.encoding = None
        if TIKTOKEN_AVAILABLE:
            try:
                self.encoding = tiktoken.encoding_for_model(model)
            except KeyError:
                self.encoding = tiktoken.get_encoding("cl100k_base")

    def count_tokens(self, text: str) -> int:
        """Estimate token count for a text string."""
        if not text:
            return 0
            
        if self.encoding:
            return len(self.encoding.encode(text))
        
        # Heuristic: ~4 chars per token for English/Code
        return math.ceil(len(text) / 4)

    def count_message_tokens(self, message: Message) -> int:
        """Count tokens in a message object (content + metadata overhead)."""
        count = self.count_tokens(message.content or "")
        
        # Add basic overhead for message structure (role, etc)
        count += 4 
        
        if message.tool_calls:
            for tc in message.tool_calls:
                 # Estimate tool call tokens
                 count += self.count_tokens(str(tc))
        
        return count

    def prune_context(self, messages: List[Message]) -> List[Message]:
        """
        Prune messages to fit within context window.
        Returns a NEW list of messages.
        """
        # 1. Calculate limits
        effective_limit = self.max_tokens - self.output_reserve - self.safety_margin
        current_tokens = sum(self.count_message_tokens(m) for m in messages)
        
        if current_tokens <= effective_limit:
            return messages

        logger.warning(f"Context overflow ({current_tokens} > {effective_limit}). Pruning...")
        
        # 2. Identify critical messages (System + Last 5)
        # We need to deep copy or create new list to avoid mutating original
        pruned_messages = list(messages)
        
        # 3. Aggressive Visual Pruning (Pillar 1 Optimization)
        # Identify and truncate old base64 screenshots to save tokens.
        # We keep only the LAST screenshot found in the history.
        base64_regex = r"data:image/[^;]+;base64,[a-zA-Z0-9+/=]+"
        last_screenshot_idx = -1
        for i, msg in enumerate(pruned_messages):
            if msg.content and re.search(base64_regex, msg.content):
                last_screenshot_idx = i
        
        if last_screenshot_idx != -1:
            for i, msg in enumerate(pruned_messages):
                if i < last_screenshot_idx and msg.content:
                    # Replace old screenshots with a placeholder
                    msg.content = re.sub(
                        base64_regex, 
                        "[SCREENSHOT REMOVED TO SAVE TOKENS - SEE ARTIFACTS]", 
                        msg.content
                    )

        # 4. Aggressive Tool Output Truncation (In-place modification of the copy)
        # This is the biggest gain: Truncate file reads > 20k chars
        MAX_TOOL_OUTPUT = 20000
        
        for msg in pruned_messages:
            if msg.role == Role.TOOL and msg.content and len(msg.content) > MAX_TOOL_OUTPUT:
                original_len = len(msg.content)
                keep_len = MAX_TOOL_OUTPUT // 2
                
                head = msg.content[:keep_len]
                tail = msg.content[-keep_len:]
                
                msg.content = (
                    f"{head}\n"
                    f"... [TRUNCATED TOOL OUTPUT: {original_len - MAX_TOOL_OUTPUT} chars removed] ...\n"
                    f"{tail}"
                )
                logger.info(f"Truncated tool output from {original_len} to {len(msg.content)} chars")

        # Recalculate
        current_tokens = sum(self.count_message_tokens(m) for m in pruned_messages)
        if current_tokens <= effective_limit:
            return pruned_messages

        # 5. Drop Middle Messages (Oldest first, excluding System and Last N)
        # Strategy: Keep Index 0 (System) and Index -5: (Recent)
        # Remove from index 1 to -5
        
        system_msg = pruned_messages[0] if pruned_messages and pruned_messages[0].role == Role.SYSTEM else None
        
        # Always protect the last 5 messages
        keep_count = 5
        if len(pruned_messages) <= keep_count:
             recent_messages = pruned_messages if not system_msg else pruned_messages[1:]
             candidates = []
        else:
             recent_messages = pruned_messages[-keep_count:]
             # If system_msg exists, candidates are between index 1 and -5
             start = 1 if system_msg else 0
             candidates = pruned_messages[start:-keep_count]

        # Drop candidates if needed
        while candidates and current_tokens > effective_limit:
            removed = candidates.pop(0)
            current_tokens -= self.count_message_tokens(removed)
            
        # If still over limit, we have to start truncating/dropping RECENT messages
        # But for now, let's just return what we have (API might reject, but better than empty)
        
        final_list = []
        if system_msg:
            final_list.append(system_msg)
        final_list.extend(candidates)
        final_list.extend(recent_messages)
        
        return final_list
