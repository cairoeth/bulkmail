## Sender

graph TD
    A[Message Added] -->|add_message| B[Queue]
    B -->|process_next_message| C{Available Slot?}
    C -->|Yes| D[Process Message]
    C -->|No| E[Wait for Slot]
    E --> C
    D --> F{Message Expired?}
    F -->|Yes| G[Drop Message]
    F -->|No| H[Get Nonce and Gas Price]
    H --> I[Send Transaction]
    I --> J[Track Pending Transaction]
    J --> K{Transaction Confirmed?}
    K -->|Yes| L[Handle Receipt]
    K -->|No| M{Transaction Dropped?}
    M -->|Yes| N{Retries Left?}
    N -->|Yes| O[Bump Transaction Fee]
    O --> I
    N -->|No| P[Log Failure]
    L --> Q[Update Gas and Nonce Trackers]
    M -->|No| R[Wait for Confirmation/Timeout]
    R --> K
