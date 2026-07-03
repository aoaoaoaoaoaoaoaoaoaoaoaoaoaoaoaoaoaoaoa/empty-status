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

- Global keys live under `[global]` (`min_polling_interval`, `padding`).
- Units are `[[units]]` tables.
- Each unit must specify:
  - `type = "..."`
  - optional `poll_interval = <seconds>`
  - plus any unit-specific keys.

Unknown keys are rejected.

Claude Code quota telemetry comes from Claude Code's `statusLine` JSON. Configure
Claude Code to let `empty-status` write the shared quota cache:

```json
{
  "statusLine": {
    "type": "command",
    "command": "/home/main/.local/bin/empty-status --empty-status-claude-statusline"
  }
}
```

Fast checks:

```bash
python3 scripts/check.py
```

Install to `~/.local/bin` (via `cargo install --root`):

```bash
python3 scripts/install.py
```
