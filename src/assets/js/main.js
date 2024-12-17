// import { invoke } from '@tauri-apps/api/core';

const { invoke } = require("@tauri-apps/api/core");

window.onload = () => {
	getRecords();
}


function convertToTime(timestamp) {
	const date = new Date(timestamp * 1000);
	const options = {
		weekday: "short",
		year: "numeric",
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
		hour12: true,
	};
	const formattedDate = date.toLocaleString("en-US", options);
	return formattedDate;
}

function isDownloadUrl(str) {
	const urlPattern = /^(https?|ftp):\/\/[^\s/$.?#].[^\s]*$/i;
	const fileExtensions = ['.pdf', '.zip', '.exe', '.mp3', '.txt', '.jpg', '.png', '.tar', '.gz'];

	// Check if it matches a URL pattern and contains a valid file extension
	return urlPattern.test(str) && fileExtensions.some(extension => str.toLowerCase().endsWith(extension));
}


async function getRecords() {
	try {
		const invoke = window.__TAURI__.core.invoke;
		const data = await invoke("fetch_records");


		let htmlBody = '';
		for (let item of data) {
			const dateTime = convertToTime(item['download_start_time']);
			const status = item['download_status'];
			const progress = 89;
			const progressClass = status == "Finished" ? "success"
				: status == "Pending" || status == "InProgress" ? "info"
					: status == "Failed" ? "danger"
						: "warning";
			const actionClass = status == "Finished" ? "primary"
				: status == "Failed" || status == "Cancelled" || status == "Pending" ? "success"
					: "warning";
			const icon = status == "Finished" ? "fa fa-folder-open"
				: status == "Failed" || status == "Cancelled" ? "fa fa-play"
					: "fa fa-pause";

			htmlBody += `
			<tr>
				<td>${item['file_name']}</td>
				<td>${item['file_size']}</td>
				<td>
					<div 
						class="progress" 
						role="progressbar" 
						aria-label="${status}"
						aria-valuenow="${progress}"
						aria-valuemax="100">
						<div 
							class="progress-bar text-bg-${progressClass}"
							style="width: 100%">
							${progress}%
						</div>
					</div>
				</td>
				<td>${status}</td>
				<td>${item['file_type']}</td>
				<td>
					<a 
						href="#" 
						class="action-link btn btn-sm btn-outline-${actionClass} btn-block"
						onclick="alert('hello')">
						<i class="${icon}"></i>
					</a>
				</td>
				<td>${dateTime}</td>
			</tr>
			`;
		}

		document.getElementById("download-records").innerHTML = htmlBody;
	} catch (error) {
		console.log(`Error: ${error}`);
	}
}

// Download 

const searchField = document.getElementById("search");
searchField.addEventListener("paste", async (event) => {
	const pastedContent = event.clipboardData.getData("text");
	if (isDownloadUrl(pastedContent)) {
		const invoke = window.__TAURI__.core.invoke;
		await invoke("download", { url: pastedContent });
	}
	searchField.value = '';

})

// End Downloads 
