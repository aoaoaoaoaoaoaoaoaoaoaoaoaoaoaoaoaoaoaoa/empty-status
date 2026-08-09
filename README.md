# empty-status

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

`empty-status` is a closed, interactive [i3bar](https://i3wm.org/docs/i3bar-protocol.html)
status line for Linux. One asynchronous reactor owns polling, clicks, rendering,
and persistence; malformed unit configuration becomes an inert red block rather
than killing its neighbors.

The built-in units cover battery, CPU, disks, memory, networking, time, weather,
Wi-Fi, and Claude/Codex/OpenRouter quota telemetry. There is deliberately no
plugin runtime.

## Install

Installation from crates.io requires Linux and Rust 1.94 or newer:

```sh
cargo install --locked empty-status
```

Then make it i3bar's status command:

```text
bar {
    status_command empty-status
}
```

The first run creates
`$XDG_CONFIG_HOME/empty-status/config.toml`, defaulting to
`~/.config/empty-status/config.toml`, with a working clock configuration.

## Configure

Units are declared from rightmost to leftmost. This is a complete minimal
configuration:

```toml
[global]
padding = 1

[[units]]
type = "Time"
poll_interval = 1.0
format = "%a %b %d %Y - %H:%M"
```

[`config.example.toml`](config.example.toml) is the complete annotated schema.
The root and every unit reject unknown keys. Expensive probes have enforced
cadence floors: 120 seconds for Weather and 15 seconds for Quota.

Available unit types:

| Type | Source |
|---|---|
| `Bat` | Linux power-supply sysfs |
| `Cpu` | CPU utilization |
| `Disk` | Linux block-device statistics |
| `Mem` | system memory utilization |
| `Net` | interface throughput and optional `ping` latency |
| `Quota` | Claude, Codex, and OpenRouter quota telemetry |
| `Time` | local time through a Chrono format string |
| `Weather` | Open-Meteo forecast and air-quality APIs |
| `Wifi` | Linux wireless netlink |

## Interact

Weather uses two independent mouse axes. Left click switches between immediate
and forecast values; right click cycles temperature, relative humidity, and
U.S. AQI. Humidity colors run from dry yellow through comfort and swamp greens,
then decay through bog brown into corpse purple. AQI keeps the EPA category
boundaries but uses the bar's continuous Base16 cold-to-hot scale, saturating
at violet for AQI 200.

Quota's left click switches between remaining quantities and reset windows.
Right click cycles configured providers, and middle click forces an immediate
refresh. Claude and Codex report percentages; OpenRouter reports remaining U.S.
dollars.

Claude subscription telemetry enters through Claude Code's supported
`statusLine` surface. Point it at the companion binary installed with
`empty-status`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "empty-status-claude-statusline"
  }
}
```

The OpenRouter source reads a management key from the absolute `token_file`
named in configuration. The key is reread for every probe and is never emitted
to the status line or log.

## System Conduct

Interaction posture survives i3 and process restarts in
`$XDG_STATE_HOME/empty-status/posture.json`. The process writes one bounded,
per-session log to `$XDG_STATE_HOME/empty-status/last.log`; the file is
truncated on startup. Missing or corrupt posture never blocks startup.

Weather performs network requests to Open-Meteo. Air-quality values use the
CAMS ENSEMBLE forecast exposed by Open-Meteo and retain the EPA's U.S. AQI
category boundaries. Net can spawn `ping`; Quota can invoke Codex app-server,
read Claude's local status-line cache, and call OpenRouter's authenticated
credits endpoint. No other unit performs external networking.

## Develop

[`SPEC.md`](SPEC.md) states the architecture and invariants. The canonical gate
formats, lints, tests, and builds documentation:

```sh
python3 scripts/check.py deep
```

Install the working tree into `~/.local/bin` after a completed change:

```sh
python3 scripts/install.py
```

## License

[MIT](LICENSE)
