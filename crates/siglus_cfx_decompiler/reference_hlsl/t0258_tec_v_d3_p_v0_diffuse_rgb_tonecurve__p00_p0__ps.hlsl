uniform sampler2D s0;

struct PS_INPUT {
    float4 v0 : COLOR0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);

    r0.w = dot(c0, input.v0);
    r1.xyz = lerp(input.v0.xyz, r0.www, c3.yyy);
    r1.w = c3.x;
    r0.x = r1.x;
    r0.y = r1.w;
    r1.x = r1.y;
    r1.y = r1.w;
    r2.x = r1.z;
    r2.y = r1.w;
    r0 = tex2D(s0, r0.xy);
    r1 = tex2D(s0, r1.xy);
    r2 = tex2D(s0, r2.xy);
    r2.x = r0.x;
    r2.y = r1.y;
    r0.xyz = lerp(r2.xyz, c1.xyz, c1.www);
    r0.xyz = (r0.xyz + c2.xyz);
    r0.w = input.v0.w;
    output.oC0 = r0;
    return output;
}
