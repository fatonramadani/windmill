<script lang="ts">
	import Tooltip from './Tooltip.svelte'

	import { slide } from 'svelte/transition'

	import { faChevronDown, faChevronUp, faMinus, faPlus } from '@fortawesome/free-solid-svg-icons'

	import StringTypeNarrowing from './StringTypeNarrowing.svelte'
	import Icon from 'svelte-awesome'
	import ResourcePicker from './ResourcePicker.svelte'
	import ObjectTypeNarrowing from './ObjectTypeNarrowing.svelte'
	import ObjectResourceInput from './ObjectResourceInput.svelte'
	import FieldHeader from './FieldHeader.svelte'

	export let label: string = ''
	export let value: any
	export let defaultValue: any = undefined
	export let description: string = ''
	export let format: string = ''
	export let contentEncoding = ''
	export let type: string | undefined = undefined
	export let required = false
	export let pattern: undefined | string
	export let valid = required ? false : true
	export let minRows = 1
	export let maxRows = 10
	export let enum_: string[] | undefined = undefined
	export let disabled = false
	export let editableSchema = false
	export let itemsType: { type?: 'string' | 'number' } | undefined = undefined
	export let displayHeader = true

	let seeEditable: boolean = enum_ != undefined || pattern != undefined

	$: minHeight = `${1 + minRows * 1.2}em`
	$: maxHeight = maxRows ? `${1 + maxRows * 1.2}em` : `auto`

	$: validateInput(pattern, value)

	let error: string = ''

	let rawValue: string | undefined

	$: {
		if (rawValue) {
			try {
				value = JSON.parse(rawValue)
			} catch (err) {
				error = err.toString()
			}
		}
	}

	$: {
		if (!type || type == 'object' || (type == 'array' && itemsType?.type == undefined)) {
			evalValueToRaw()
		}
		if (defaultValue) {
			let stringified = JSON.stringify(defaultValue, null, 4)
			if (stringified.length > 50) {
				minRows = 3
			}
			if (type != 'string') {
				minRows = Math.max(minRows, Math.min(stringified.split(/\r\n|\r|\n/).length + 1, maxRows))
			}
		}
	}

	export function evalValueToRaw() {
		rawValue = JSON.stringify(value, null, 4)
	}

	function fileChanged(e: any) {
		let t = e.target
		if (t && 'files' in t && t.files.length > 0) {
			let reader = new FileReader()
			reader.onload = (e: any) => {
				value = e.target.result.split('base64,')[1]
			}
			reader.readAsDataURL(t.files[0])
		} else {
			value = undefined
		}
	}

	function validateInput(pattern: string | undefined, v: any): void {
		if (required && v == undefined) {
			error = 'This field is required'
			valid = false
		} else {
			if (pattern && !testRegex(pattern, v)) {
				error = `Should match ${pattern}`
				valid = false
			} else {
				error = ''
				valid = true
			}
		}
	}

	function testRegex(pattern: string, value: any): boolean {
		try {
			const regex = new RegExp(pattern)
			return regex.test(value)
		} catch (err) {
			return false
		}
	}

	$: {
		if (value == undefined) {
			value = defaultValue
		}
	}
</script>

<div class="flex flex-col w-full">
	<div>
		{#if displayHeader}
			<FieldHeader {label} {required} {type} {contentEncoding} {format} {itemsType} />
		{/if}
		{#if editableSchema}
			<div class="my-1 text-xs border-solid border border-gray-400 rounded p-2">
				<span
					class="underline"
					on:click={() => {
						seeEditable = !seeEditable
					}}
					>Customize argument<Icon
						class="ml-2"
						data={seeEditable ? faChevronUp : faChevronDown}
						scale={0.7}
					/></span
				>

				{#if seeEditable}
					<div transition:slide class="mt-2">
						<label class="text-gray-700"
							>Description
							<textarea rows="1" bind:value={description} placeholder="Edit description" />
							{#if type == 'string' && !contentEncoding && format != 'date-time'}
								<StringTypeNarrowing bind:format bind:pattern bind:enum_ />
							{:else if type == 'object'}
								<ObjectTypeNarrowing bind:format />
							{:else if type == 'array'}
								<select bind:value={itemsType}>
									<option value={undefined}>No specific item type</option>
									<option value={{ type: 'string' }}> Items are strings</option>
									<option value={{ type: 'number' }}>Items are numbers</option>
								</select>
							{/if}
						</label>
					</div>
				{/if}
			</div>
			<span class="text-2xs">Preview:</span>
		{/if}

		<div class="grid grid-cols-2">
			<div class="text-sm italic pb-1">
				{description}
			</div>
			<div class="text-right text-xs {error === '' ? 'text-white' : 'font-bold text-red-600'}">
				{error === '' ? '...' : error}
			</div>
		</div>
		<div class="container">
			{#if type == 'number' || type == 'integer'}
				<input
					{disabled}
					type="number"
					class={valid
						? ''
						: 'border border-red-700 border-opacity-30 focus:border-red-700 focus:border-opacity-30 bg-red-100'}
					placeholder={defaultValue}
					bind:value
				/>
			{:else if type == 'boolean'}
				<input
					{disabled}
					type="checkbox"
					class={valid
						? ''
						: 'border border-red-700 border-opacity-30 focus:border-red-700 focus:border-opacity-30 bg-red-100'}
					bind:checked={value}
				/>
				{#if type == 'boolean' && value == undefined}
					<span>&nbsp; Not set</span>
				{/if}
			{:else if type == 'array' && itemsType?.type != undefined}
				{#each value ?? [] as v}
					<div class="flex flex-row max-w-md">
						{#if itemsType.type == 'number'}
							<input type="number" bind:value={v} />
						{:else}
							<input type="text" bind:value={v} />
						{/if}
						<button
							class="default-button-secondary mx-6"
							on:click={() => {
								value = value.filter((el) => el != v)
								if (value.length == 0) {
									value = undefined
								}
							}}><Icon data={faMinus} class="mb-1" /></button
						>
					</div>
				{/each}
				<button
					class="default-button-secondary mt-1"
					on:click={() => {
						if (value == undefined) {
							value = []
						}
						value = value.concat('')
					}}>Add item &nbsp;<Icon data={faPlus} class="mb-1" /></button
				><span class="ml-2">{(value ?? []).length} item(s)</span>
			{:else if type == 'object' && format?.startsWith('resource')}
				<ObjectResourceInput {format} bind:value />
			{:else if !type || type == 'object' || type == 'array'}
				<textarea
					{disabled}
					style="min-height: {minHeight}; max-height: {maxHeight}"
					class="col-span-10 {valid
						? ''
						: 'border border-red-700 border-opacity-30 focus:border-red-700 focus:border-opacity-30 bg-red-100'}"
					placeholder={JSON.stringify(defaultValue, null, 4)}
					bind:value={rawValue}
				/>
			{:else if type == 'string' && enum_}
				<select {disabled} class="px-6" bind:value>
					{#each enum_ as e}
						<option>{e}</option>
					{/each}
				</select>
			{:else if type == 'string' && format == 'date-time'}
				<input class="inline-block" type="datetime-local" bind:value />
			{:else if type == 'string' && contentEncoding == 'base64'}
				<input type="file" class="my-6" on:change={fileChanged} multiple={false} />
			{:else if type == 'string' && format?.startsWith('resource')}
				<ResourcePicker
					bind:value
					resourceType={format.split('-').length > 1
						? format.substring('resource-'.length)
						: undefined}
				/>
			{:else}
				<textarea
					{disabled}
					style="height: {minHeight}; max-height: {maxHeight}"
					class="col-span-10 {valid
						? ''
						: 'border border-red-700 border-opacity-30 focus:border-red-700 focus:border-opacity-30 bg-red-100'}"
					placeholder={defaultValue}
					bind:value
				/>
			{/if}
			{#if !required}
				<div class="flex flex-row-reverse">
					<button
						{disabled}
						class="default-button-secondary items-center leading-4 py-0 my-px px-1 float-right"
						on:click={() => (value = undefined)}
						>Reset<Tooltip class="pl-1" position={'above'} direction={'left'}
							>Reset to default value
						</Tooltip></button
					>
				</div>
			{/if}
		</div>
	</div>
</div>

<style>
</style>
