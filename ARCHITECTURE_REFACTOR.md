# Architecture Refactor: Idiomatic Rust Design

## Current Issues

1. **Domain/Infrastructure Split**: Not idiomatic Rust - creates artificial boundaries
2. **#[cfg(simulation)]**: Runtime config would be more flexible and allow same traits
3. **Over-abstraction**: Too many service layers without clear benefit

## Proposed Idiomatic Structure

```
src/
├── lib.rs                  # Public API, main types
├── config.rs              # Configuration with simulation modes
├── 
├── torrent/               # BitTorrent protocol implementation
│   ├── mod.rs            # Public API, TorrentEngine
│   ├── engine.rs         # Main torrent engine
│   ├── peer/             # Peer management
│   ├── tracker/          # Tracker communication  
│   └── protocol/         # BitTorrent protocol details
│
├── storage/              # File and data storage
│   ├── mod.rs           # Storage traits and main types
│   ├── file_storage.rs  # Local file storage
│   └── simulated.rs     # Simulated storage (when config.simulation = true)
│
├── streaming/            # Media streaming server
│   ├── mod.rs           # Streaming service API
│   ├── server.rs        # HTTP streaming server
│   └── coordinator.rs   # Stream coordination
│
├── search/              # Media search and discovery  
│   ├── mod.rs           # Search API and traits
│   ├── providers/       # Search provider implementations
│   └── client.rs        # Search client coordination
│
├── media/               # Media management and catalog
│   ├── mod.rs           # Media types and catalog
│   ├── catalog.rs       # Media catalog
│   └── metadata.rs      # Metadata handling
│
├── web/                 # Web UI and API
│   ├── mod.rs           # Web server setup
│   ├── handlers.rs      # Request handlers
│   └── templates.rs     # Template engine
│
└── simulation/          # Simulation utilities (no #[cfg])
    ├── mod.rs           # Simulation mode utilities
    ├── network.rs       # Network simulation
    └── scenarios.rs     # Test scenarios
```

## Key Principles

### 1. Runtime Configuration Instead of Compile-Time Features
```rust
#[derive(Clone)]
pub struct RiptideConfig {
    pub simulation_mode: bool,
    pub simulation_settings: SimulationSettings,
    // ... other config
}

// Instead of #[cfg(simulation)]
impl TorrentEngine {
    pub fn new(config: RiptideConfig) -> Self {
        let peer_manager = if config.simulation_mode {
            Box::new(SimulatedPeerManager::new())
        } else {
            Box::new(RealPeerManager::new())
        };
        // ...
    }
}
```

### 2. Trait-Based Architecture with Dynamic Dispatch
```rust
pub trait PeerManager: Send + Sync {
    async fn connect_to_peer(&mut self, peer: Peer) -> Result<()>;
    async fn disconnect_peer(&mut self, peer_id: PeerId) -> Result<()>;
}

pub struct RealPeerManager { /* ... */ }
pub struct SimulatedPeerManager { /* ... */ }

impl PeerManager for RealPeerManager { /* real implementation */ }
impl PeerManager for SimulatedPeerManager { /* simulated implementation */ }
```

### 3. Clear Module Ownership
- Each module owns its data and provides focused APIs
- No artificial domain/infrastructure boundaries
- Dependencies flow downward (web -> torrent -> storage)

### 4. Simulation as First-Class Feature
```rust
pub struct SimulationSettings {
    pub network_latency_ms: u64,
    pub packet_loss_rate: f64,
    pub max_peers: usize,
    pub deterministic_seed: Option<u64>,
}

pub fn create_simulated_engine(settings: SimulationSettings) -> TorrentEngine {
    let config = RiptideConfig {
        simulation_mode: true,
        simulation_settings: settings,
        ..Default::default()
    };
    TorrentEngine::new(config)
}
```

## Migration Plan

1. **Phase 1**: Keep existing structure, add runtime simulation config
2. **Phase 2**: Move simulation code to be runtime-configurable
3. **Phase 3**: Flatten domain/infrastructure into focused modules
4. **Phase 4**: Update all trait implementations to work with both modes
5. **Phase 5**: Remove #[cfg(simulation)] feature flag

## Benefits

- **Flexibility**: Switch between real/simulated at runtime
- **Testing**: Same traits for both modes = better test coverage
- **Performance**: No compilation overhead for unused features
- **Maintainability**: Clear module boundaries, idiomatic Rust patterns
- **Debugging**: Can run simulated components in production for testing