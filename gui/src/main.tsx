import "@mantine/core/styles.css";
import { MantineProvider, createTheme } from "@mantine/core";
import { createRoot } from "react-dom/client";
import App from "./App";

const theme = createTheme({
	primaryColor: "yellow",
});

const root = document.getElementById("root");
if (root) {
	createRoot(root).render(
		<MantineProvider theme={theme} defaultColorScheme="dark">
			<App />
		</MantineProvider>,
	);
}
