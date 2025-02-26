<script lang="ts">
	import { goto } from '$app/navigation'
	import { page } from '$app/stores'
	import { logout, logoutWithRedirect, sendUserToast } from '../../utils'
	import { UserService, type WorkspaceInvite, WorkspaceService } from '../../gen'
	import { superadmin, usersWorkspaceStore, workspaceStore } from '../../stores'
	import CenteredModal from './CenteredModal.svelte'
	import Switch from '../components/Switch.svelte'
	import { faCrown, faUser, faUserAlt, faUserCog } from '@fortawesome/free-solid-svg-icons'
	import Icon from 'svelte-awesome'

	let invites: WorkspaceInvite[] = []
	let list_all_as_super_admin: boolean = false
	let workspaces: { id: string; name: string; username: string }[] = []

	async function loadInvites() {
		invites = await UserService.listWorkspaceInvites()
	}

	async function loadWorkspaces() {
		if (!$usersWorkspaceStore) {
			try {
				usersWorkspaceStore.set(await WorkspaceService.listUserWorkspaces())
			} catch {}
		}
		if ($usersWorkspaceStore) {
			if (!$workspaceStore) {
				workspaceStore.set(localStorage.getItem('workspace')?.toString())
			}
		} else {
			logoutWithRedirect($page.url.pathname)
		}
	}

	async function loadWorkspacesAsAdmin() {
		workspaces = (await WorkspaceService.listWorkspacesAsSuperAdmin({ perPage: 1000 })).map((x) => {
			return { ...x, username: 'superadmin' }
		})
	}

	$: {
		if (list_all_as_super_admin) {
			loadWorkspacesAsAdmin()
		} else {
			workspaces = $usersWorkspaceStore?.workspaces ?? []
		}
	}

	loadInvites()
	loadWorkspaces()
</script>

<CenteredModal title="Select a workspace" subtitle="Logged in as {$usersWorkspaceStore?.email}">
	<h2 class="mb-4">Workspaces</h2>
	{#if $superadmin}
		<Switch
			textFormat="text-xs"
			label={'List all as superadmin'}
			bind:checked={list_all_as_super_admin}
		/>
		<div class="my-4" />
	{/if}
	{#if workspaces.length == 0}
		<p class="text-xs mt-2">
			You are not a member of any workspace yet. Accept an invitation or create your own workspace.
		</p>
	{/if}
	{#each workspaces as workspace}
		<label class="block pb-2">
			<button
				class="
					block
					w-96
					mx-auto
					py-1
					px-2
					rounded-md
					border
					border-gray-300
					shadow-sm
					text-sm
					mt-1
					hover:ring-1
					hover:ring-indigo-300
					"
				on:click={() => {
					workspaceStore.set(workspace.id)
					goto('/')
				}}
				><span class="font-mono">{workspace.id}</span> - {workspace.name} as
				<span class="font-mono">{workspace.username}</span>
			</button>
		</label>
	{/each}
	<div class="flex flex-row-reverse  pt-4">
		<a href="/user/create_workspace" class="primary-button">Create a new workspace &rightarrow; </a>
	</div>
	<h2 class="my-2">Invitations</h2>
	<p class="text-xs mb-2 italic text-gray-500">Join a workspace</p>

	{#if invites.length == 0}
		<p class="text-xs mt-2">You have no invites to any workspaces at the moment.</p>
	{/if}
	{#each invites as invite}
		<div
			class="
					block
					w-96
					mx-auto
					py-1
					px-2
					rounded-md
					border
					border-gray-300
					shadow-sm
					text-sm
					mt-1
					flex flex-row
					justify-around
					items-center
					"
		>
			<span class="text-xl">
				{invite.workspace_id}
				{#if invite.is_admin}
					<span class="text-sm">as an admin</span>
				{/if}
			</span>
			<span>
				<a href="/user/accept_invite?workspace={encodeURIComponent(invite.workspace_id)}">
					accept
				</a>

				<button
					class="ml-2"
					on:click={async () => {
						await UserService.declineInvite({
							requestBody: { workspace_id: invite.workspace_id }
						})
						sendUserToast(`Declined invite to ${invite.workspace_id}`)
						loadInvites()
					}}
				>
					decline
				</button>
			</span>
		</div>
	{/each}
	<div class="flex justify-between mt-10">
		{#if $superadmin}
			<a class="mr-10" href="/user/superadmin_settings">
				<Icon data={faCrown} class="mr-1" scale={1} />Superadmin settings</a
			>
		{/if}
		<a class="mr-10" href="/user/settings">
			<Icon data={faUserCog} class="mr-1" scale={1} />User settings</a
		>
		<button
			class="default-button-secondary"
			on:click={async () => {
				logout()
			}}
		>
			logout
		</button>
	</div>
</CenteredModal>
