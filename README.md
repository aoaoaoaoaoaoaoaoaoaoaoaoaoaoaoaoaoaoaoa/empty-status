```
                                 i3i3i3i                                        
                                 i3i3i3i3i3                                     
                                  i3i3i3i3i3i                                   
                                   i3i3i3i3i3                                   
                                    i3i3i3i3i3                                  
                                    i3i3i3i3i3                                  
                                     i3i3i3i3i                                  
                                      i3i3i3i3                                  
                                       i3i3i3i                                  
                                        i3i3i3                                  
                                         i3i3                i3i                
                                                          i3i3i3i3i             
               i3i                                 i3i3i3i3i3i3i3i3i            
              i3i3i                         i3i3i3i3i3i3i3i3i3i3i3i3i           
              i3i3i3                   i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i        
              i3i3i3             i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3       
              i3i3i3      i3i3i3i3i3i3i3i3i3i3i3i3i3i             i3i3i3i3i3i3  
             i3i3i3i3i3i3i3i3i3i3i3i3i3i                     i3i3i3i3i3i3i3i    
             i3i3i3i3i3i3i3i3i3i3                           i3i3i3i3i3i3i3i     
            i3i3i3i3i3i3i3i3                                i3i3i3i3i3i3i3i     
            i3i3i3  i3i                                    i3i3i3i3i3i3i3       
           i3i3i3i                                         i3i3i3i3i3i3i        
           i3i3i3i                                        i3i3i3i3i3i3          
          i3i3i3i3                                        i3i3i3i3i3i           
         i3i3i3i3           i3i3i           i3i            i3i3                 
         i3i3i3i3           i3i3i3i         i3i3i3i        i3                   
         i3i3i3i            i3i3i3i3         i3i3i3i3i                          
        i3i3i3i3            i3i3i3i3i         i3i3i3i3i3                        
        i3i3i3i             i3i3i3i3i          i3i3i3i3i3                       
        i3i3i3             i3i3i3i3i            i3i3i3i3i3                      
         i3i3i             i3i3i3i3              i3i3i3i3i3                     
         i3i3              i3i3i3i                i3i3i3i3i3                    
          i               i3i3i3i                  i3i3i3i3i                    
                         i3i3i3i                    i3i3i3i3                    
                         i3i3i3                       i3i3i3                    
                        i3i3i3                         i3i3i                    
                       i3i3i3                            i3i                    
                      i3i3i3                                                    
                     i3i3i3                                                     
                    i3i3i3                                                      
                   i3i3i3                                                       
                  i3i3i3                        i3i3i                           
                 i3i3i                     i3i3i3i3i3i3i                        
                i3i3i                 i3i3i3i3i3i3i3i3i3i                       
              i3i3i              i3i3i3i3i3i3i3i3i3i3i3i3                       
              i3i       i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3                        
                       i3i3i3i3i3i3i3i3i3i3i3i3i3i3                             
                         i3i3i3i3i3i3i3i3i                                      
                           i3i3i3i3  i3i3i                                      
                                     i3i3i3i                                    
                                     i3i3i3i                                    
                                      i3i3i3                                    
                                      i3i3i3                                    
                                      i3i3i3                                    
                                      i3i3i3                                    
                                      i3i3i3                                    
                                      i3i3i                                    
                                      i3i3i                                    
                                      i3i3i                                    
                                      i3i3i                                    
                                      i3i3i             i3i3i3i3i3i            
                                      i3i3i      i3i3i3i3i3i3i3i3i3i3          
                                      i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3        
                                i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i       
                       i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3       
              i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3      
        i3i3i3i3i3i3i3i3i3i3i3i3i3i3i3                         i3i3i3i3i3       
         i3i3i3i3i3i3i3i3i3i3i                                        i3        
          i3i3i3i3i3i3i3i                                                       
            i3i3i3i3i                                                           
              i3i                                                               
```

## Dev

Design is specified in `SPEC.md`. This document is the authoritative, living
description of the current architecture and invariants.

## Config

Config lives at `$XDG_CONFIG_HOME/empty-status/config.toml`.

Schema:

- The sole global key is `[global].padding`.
- Units are `[[units]]` tables.
- Each unit must specify:
  - `type = "..."`
  - optional `poll_interval = <seconds>`
  - plus any unit-specific keys.

Unknown keys are rejected. Unit stanzas are ordered from rightmost to leftmost.
Weather polls no faster than 120 seconds and Quota no faster than 15 seconds;
their default cadence is 300 seconds. `config.example.toml` is the complete
normative schema.

Weather's left mouse button cycles immediate/forecast while the right button
cycles temperature/relative humidity/U.S. AQI, producing six independent
display states. Relative humidity is Open-Meteo's 2 m instantaneous forecast
value. AQI is the consolidated U.S. index from Open-Meteo's global
[Air Quality API](https://open-meteo.com/en/docs/air-quality-api); it is a CAMS
model forecast rather than a nearby AirNow station observation. Values retain
the [EPA's U.S. AQI category boundaries](https://www.epa.gov/outdoor-air-quality-data/airdata-basic-information),
but colors use the bar's continuous Base16 cold-to-hot scale and saturate at
violet for AQI 200.

Air-quality data attribution: Open-Meteo and CAMS ENSEMBLE data provided by the
Copernicus Atmosphere Monitoring Service.

Quota's right mouse button cycles the configured providers; its left button
toggles remaining quantities and reset windows. Provider order is configuration
order. Middle click forces an immediate refresh. Claude and Codex report
percentages, while OpenRouter reports remaining U.S. dollars:

```toml
providers = [
  { source = "claude" },
  { source = "codex" },
  { source = "openrouter", token_file = "/absolute/path/to/openrouter-management-key" },
]
```

Claude Code subscription telemetry comes from its supported `statusLine` JSON
surface. Configure Claude Code to let `empty-status` write the shared quota
cache:

```json
{
  "statusLine": {
    "type": "command",
    "command": "/home/main/.local/bin/empty-status-claude-statusline"
  }
}
```

The OpenRouter source calls `GET /api/v1/credits`. Its `token_file` must contain
an OpenRouter management key and must be an absolute path. The file is read at
each poll; its contents never appear in configuration text or logs.

Fast checks:

```bash
python3 scripts/check.py
```

Install to `~/.local/bin` (via `cargo install --root`):

```bash
python3 scripts/install.py
```
