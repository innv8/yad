
window.onload = () => {
	getRecords();
}


function convertToTime(timestamp) {
	const date = new Date(timestamp * 1000);
	const options = {
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
		hour12: true,
	};
	const formattedDate = date.toLocaleString("en-GB", options);
	return formattedDate;
}

function getSize(size) {
	let unit = '';
	let finalSize = 0;

	if (size < 1024) {
		finalSize = size;
		unit = 'Bytes';
	} else if (size < 1048576) {
		finalSize = size/ 1024;
		unit= 'KB';
	} else if (size < 1073741824){
		finalSize = size / 1048576;
		unit = 'MB';
	} else {
		finalSize = size / (1073741824 * 1024);
		unit = 'GB';
	}
	return `${Math.round(finalSize * 10)/ 10} ${unit}`;
}

function isDownloadUrl(str) {
	const urlPattern = /^(https|http|ftp|ftps){1}:\/\/([a-zA-Z\._0-9-\/]+)\/[a-zA-Z0-9\._-]+\.[a-z0-9]/;
	return urlPattern.test(str);
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
				<td>${getSize(item['file_size'])}</td>
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
				<td>${item['file_type']}</td>
				<td>
					<a 
						href="#" 
						class="action-link btn btn-sm btn-outline-${actionClass} btn-block"
						onclick="alert('hello')">
						<i class="${icon}"></i>
					</a>
				</td>
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
		getRecords();
	} else {
		console.log(`invalid url: ${pastedContent}`);
	}

	searchField.value = '';

})
// End Downloads 
