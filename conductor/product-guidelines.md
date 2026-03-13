# Product Guidelines: MAG

## Interaction Design & Tone
- **Primary Tone:** Professional & Efficient. Interaction should be concise, clear, and focused on task completion.
- **Progressive Disclosure:** Use a "Professional & Efficient" style for standard command outputs. For help commands and detailed explanations, provide more "Helpful & Conversational" context (e.g., in greyed-out or bracketed text) to explain *why* and *when* to use specific features.
- **Hands-Free Operation:** Once configured, the system should operate autonomously with minimal user intervention.

## Design Principles
- **Modularity First:** The system is built around a "Sensible Modular Codebase." Every step of the memory pipeline (ingestion, processing, retrieval, storage) must be a clearly defined, swappable module.
- **Interrogatability:** While the system should be quiet and "hands-free" by default, it must be highly interrogatable. Users should be able to query the system's state, logs, and decision-making processes when needed.
- **User-Centric Safety:** Leverage Rust's safety guarantees to ensure data integrity and system resilience. Prevent data loss and handle malformed inputs gracefully.
- **Portability:** Maintain a zero-dependency requirement for the end-user (aside from the binary itself).

## Quality Standards
- **Performance:** Aim for sub-millisecond overhead for core memory retrieval actions.
- **Reliability:** 100% adherence to Rust's memory safety principles.
- **Documentation:** Every module must have clear, concise documentation explaining its role in the pipeline.
