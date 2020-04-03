# StreamRecorder

Record multiple streams at once for archival purposes.

## Sample Configuration

Current the three paths must not be identical and must be escaped. Make sure FFMpeg and Streamlink are accessible in your path 
before running the application.

A config.json must be present in the working directory containing the information on the streams you wish to monitor. This can be updated during application run-time but must always be a valid file. Paths will not update for already recording streams.

```json
{
    "client_id": "xxxx",
    "client_secret": "xxxx",
    "login_names": [
        "login_name_1",
        "login_name_2",
    ],
    "recording_path": "G:\\Recording\\",
    "cleanup_path": "G:\\Temporary\\",
    "move_path": "F:\\Processed\\",
    "halt_until_next_live": false,
    "halt_newly_added": false
}
```