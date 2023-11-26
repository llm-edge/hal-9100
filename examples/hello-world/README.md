
 ________  ________   ________  ___  ________  _________  ________  ________   _________  ________      
|\   __  \|\   ____\ |\   ____\|\  \|\   ____\|\___   ___\\   __  \|\   ___  \|\___   ___\\   ____\     
\ \  \|\  \ \  \___|_\ \  \___|\ \  \ \  \___|\|___ \  \_\ \  \|\  \ \  \\ \  \|___ \  \_\ \  \___|_    
 \ \   __  \ \_____  \\ \_____  \ \  \ \_____  \   \ \  \ \ \   __  \ \  \\ \  \   \ \  \ \ \_____  \   
  \ \  \ \  \|____|\  \\|____|\  \ \  \|____|\  \   \ \  \ \ \  \ \  \ \  \\ \  \   \ \  \ \|____|\  \  
   \ \__\ \__\____\_\  \ ____\_\  \ \__\____\_\  \   \ \__\ \ \__\ \__\ \__\\ \__\   \ \__\  ____\_\  \ 
    \|__|\|__|\_________\\_________\|__|\_________\   \|__|  \|__|\|__|\|__| \|__|    \|__| |\_________\
             \|_________\|_________|   \|_________|                                         \|_________|
                                                                                                        
						___                          ___                          ___     
						/__/\                        /__/\                        /__/\    
						\  \:\                       \  \:\                       \  \:\   
						\__\:\                       \__\:\                       \__\:\  
						/  /::\                      /  /::\                      /  /::\ 
						/  /:/\:\                    /  /:/\:\                    /  /:/\:\
						/  /:/__\/                   /  /:/__\/                   /  /:/__\/
					/__/:/                       /__/:/                       /__/:/     
					\__\/                        \__\/                        \__\/      
		
At the moment you need both Rust and Docker installed to run the API.

```bash
# Start the server
make all

# Create an Assistant
curl -X POST http://localhost:3000/assistants \
-H "Content-Type: application/json" \
-d '{
    "id": 1,
    "instructions": "You are a personal math tutor. Write and run code to answer math questions.",
    "name": "Math Tutor",
    "tools": ["retrieval"],
    "model": "claude-2.1",
    "user_id": "user1"
}'

# Create a Thread
curl -X POST http://localhost:3000/threads \
-H "Content-Type: application/json"

# Add a Message to a Thread
# Replace 1 with the actual thread id
curl -X POST http://localhost:3000/threads/1/messages \
-H "Content-Type: application/json" \
-d '{
    "role": "user",
    "content": "I need to solve the equation `3x + 11 = 14`. Can you help me?"
}'

# Run the Assistant
# Replace :thread_id and :assistant_id with the actual thread id and assistant id
curl -X POST http://localhost:3000/threads/:thread_id/runs \
-H "Content-Type: application/json" \
-d '{
    "assistant_id": :assistant_id,
    "instructions": "Please solve the equation."
}'

# Check the Run Status
# Replace :thread_id and :run_id with the actual thread id and run id
curl -X GET http://localhost:3000/threads/:thread_id/runs/:run_id \
-H "Content-Type: application/json"

# Display the Assistant's Response
# Replace :thread_id with the actual thread id
curl -X GET http://localhost:3000/threads/:thread_id/messages \
-H "Content-Type: application/json"
``` 


