uniform sampler2D s0;
uniform sampler2D s1;

static const float4 c3 = float4(-2.0, 1.0, -1.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t1 : TEXCOORD1;
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
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r4 = float4(0.0, 0.0, 0.0, 0.0);

    r0.w = dot(c0, input.v0);
    r1.xyz = lerp(input.v0.xyz, r0.www, c2.yyy);
    r1.w = c2.x;
    r0.x = r1.x;
    r0.y = r1.w;
    r1.x = r1.y;
    r1.y = r1.w;
    r2.x = r1.z;
    r2.y = r1.w;
    r3 = tex2D(s1, r0.xy);
    r1 = tex2D(s1, r1.xy);
    r2 = tex2D(s1, r2.xy);
    r4 = tex2D(s0, input.t1.xy);
    r2.x = r3.x;
    r2.y = r1.y;
    r0.xyz = (r2.xyz * c3.xxx + c3.yyy);
    r0.xyz = (c1.yyy * r0.xyz + r2.xyz);
    r1.xyz = lerp(r0.xyz, r0.www, c1.xxx);
    r0.xyz = (r1.xyz + c1.zzz);
    r0.xyz = (r0.xyz + -(c1.www));
    r0.xyz = (r0.xyz + c3.zzz);
    r0.xyz = (input.v0.www * r0.xyz + c3.yyy);
    r0.w = input.v0.w;
    r0 = (r4 * r0);
    output.oC0 = r0;
    return output;
}
