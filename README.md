
### vision 

Full-stack LLM platform for developing, collaborating, testing, deploying Generate UI LLM applications in environment with a single AI accelerator (less than 10^15 FLOPS). Designed to be independent of the cloud.

### status

not usable 

### usage 

#### nextjs (client side webgpu)

```ts 
import { useState, useEffect } from 'react';
import { hal-9100 } from 'hal-9100';

const assistantConfig = {
  name: "boAt",
  instructions: "you are a bot that notify the boat's driver when a boat is nearby based on anti collision system information that you can pull from the boat antennas",
  model: "llama3"
};

const HomePage = () => {
  const [assistant, setAssistant] = useState(null);
  const [response, setResponse] = useState(null);
  const [componentProps, setComponentProps] = useState({ type: "default", data: {} });

  useEffect(() => {
    const createAssistant = async () => {
      const newAssistant = await hal-9100.assistants(assistantConfig).create();
      setAssistant(newAssistant);
    };
    createAssistant();
  }, []);

  const chat = async (message) => {
    if (assistant) {
      const res = await assistant.chat([{ role: "user", content: message }]);
      setResponse(res);
      setComponentProps(res.content); // Assuming the response content includes type and data
    }
  };

  const renderComponent = () => {
    switch (componentProps.type) {
      case "boat":
        return <BoatAlert data={componentProps.data} />;
      case "weather":
        return <WeatherAlert data={componentProps.data} />;
      default:
        return <DefaultAlert />;
    }
  };

  return (
    <div>
      <button onClick={() => chat("is there a boat nearby?")}>Ask Assistant</button>
      {response && <p>{response.content}</p>}
      {renderComponent()}
    </div>
  );
};

const BoatAlert = ({ data }) => <div>Boat Alert: {data.message}</div>;
const WeatherAlert = ({ data }) => <div>Weather Alert: {data.message}</div>;
const DefaultAlert = () => <div>No specific alert.</div>;

export default HomePage;
```


### remote server ts 

todo 

### rust 

todo

