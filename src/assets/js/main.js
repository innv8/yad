
window.onload = () => {
	getRecords();
}

function logInfo(message) {
	let now = performance.now() * 1e6;
	console.log(`${now} : ${message}`);
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
		finalSize = size / 1024;
		unit = 'KB';
	} else if (size < 1073741824) {
		finalSize = size / 1048576;
		unit = 'MB';
	} else {
		finalSize = size / (1073741824 * 1024);
		unit = 'GB';
	}
	return `${Math.round(finalSize * 10) / 10} ${unit}`;
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
			const status = item['download_status'];
			const progress = status == "Finished" ? 100 : 50;
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
			<tr id="${item['id']}">
				<td>${item['file_name']}</td>
				<td id="size=${item['id']}">${getSize(item['file_size'])}</td>
				<td id="progress-${item['id']}">
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
					<span 
						class="action-link btn btn-sm btn-outline-${actionClass} btn-block"
						id="${item['id']}"
						>
						<i class="${icon}"></i>
					</span>
</td>
			</tr>
			`;
		}

		document.getElementById("download-records").innerHTML = htmlBody;
		const actionLinkElements = document.getElementsByClassName("action-link");
		for (let i = 0; i < actionLinkElements.length; i++) {
			actionLinkElements[i].addEventListener("click", function() {
				let path = data[i]['destination_path'];
				const invoke = window.__TAURI__.core.invoke;
				invoke("open_file", { path });
			})
		}
	} catch (error) {
		logInfo(`Error: ${error}`);
	}
}



// Download 

const searchField = document.getElementById("search");
searchField.addEventListener("paste", async (event) => {
	const pastedContent = event.clipboardData.getData("text");
	if (isDownloadUrl(pastedContent)) {
		const invoke = window.__TAURI__.core.invoke;
		await invoke("download", { url: pastedContent });
		// getRecords();
	} else {
		logInfo(`invalid url: ${pastedContent}`);
	}

	searchField.value = '';
});



window.__TAURI__.event.listen("download-started", (event) => {
	// add a download record to the table
	const data = event.payload;


	let row = document.createElement("tr");
	row.id = data['downloadId'];

	let cellFileName = document.createElement("td");
	cellFileName.textContent = data['fileName'];
	row.appendChild(cellFileName);

	let cellFileSize = document.createElement("td");
	cellFileSize.id = `size-${data['downloadId']}`;
	cellFileSize.textContent = '0B';
	row.appendChild(cellFileSize);

	let cellProgressBar = document.createElement("td");
	cellProgressBar.id = `progress-${data['downloadId']}`;
	cellProgressBar.innerHTML = `
	<div
		class="progress"
		role="progressbar"
		aria-label="Pending"
		aria-valuenow="0"
		aria-valuemax="100">
		<div
			class="progress-bar text-bg-info" style="width: 100%">
			0%
		</div>
	</div>
	`
	row.appendChild(cellProgressBar);

	let cellFileType = document.createElement("td");
	cellFileType.textContent = data['fileType'];
	row.appendChild(cellFileType);

	let cellAction = document.createElement("td");
	row.appendChild(cellAction);

	let tBody = document.getElementById("download-records");
	tBody.insertBefore(row, tBody.firstChild);
	logInfo("Download Started");
});


window.__TAURI__.event.listen("download-progress", (event) => {
	const data = event.payload;
	logInfo(`download-progress:: ${JSON.stringify(data)}`);

	let percentage = (data['downloaded'] / data['totalSize']) * 100;
	percentage = percentage.toFixed(2);

	logInfo(`downloaded:: ${percentage}%`);

	let progressCell = document.getElementById(`progress-${data['downloadId']}`);
	let status = percentage == 100 ? "success" : "info";

	progressCell.innerHTML = `
	<div
		class="progress"
		role="progressbar"
		aria-label="Pending"
		aria-valuenow="${percentage}"
		aria-valuemax="100">
		<div
			class="progress-bar text-bg-${status}" style="width: 100%">
			${percentage}%
		</div>
	</div>
	`;

	if (percentage == 100) {
		logInfo("download ended, fetch");
		getRecords();
	}

	let sizeCell = document.getElementById(`size-${data['downloadId']}`);
	sizeCell.innerHTML = `${getSize(currentSize)} / ${getSize(totalSize)}`;

});


// End Downloads 

function toggleTheme() {
	let currentTheme = localStorage.getItem("theme");
	console.log(`theme: ${currentTheme}`);
	currentTheme = currentTheme == "dark" ? "light" : "dark";

	if (currentTheme == "dark") {

	}

	console.log(`new theme: ${currentTheme}`);
}

window.addEventListener("DOMContentLoaded", () => {
	console.log(`loaded!!`);
	const darkButton = document.getElementById("dark-button");
	const lightButton = document.getElementById("light-button");

	let currentTheme = localStorage.getItem("theme");
	if (currentTheme == "dark") {
		document.body.classList.add("dark-theme");
		darkButton.style.display = "none";
	} else {
		document.body.classList.remove("dark-theme");
		lightButton.style.display = "none";
	}

	darkButton.addEventListener("click", function() {
		document.body.classList.add("dark-theme");
		localStorage.setItem("theme", "dark");
		darkButton.style.display = "none";
		lightButton.style.display = "block";
	});

	lightButton.addEventListener("click", function() {
		document.body.classList.remove("dark-theme");
		localStorage.setItem("theme", "light");
		lightButton.style.display = "none";
		darkButton.style.display = "block";
	});

})
